//! F0 density spike: loads N copies of a guest as distinct functions on one of several engines
//! and measures memory, compile, first-call, steady-call and noisy-neighbour behaviour.
//! Throwaway measurement code — see docs/function-runner-density.md.

mod eng_comp;
mod eng_extism;
mod eng_kernel;
mod policy;
mod util;
mod wasi_out;

use anyhow::{bail, Context, Result};
use policy::{EgressPolicy, FnCtx};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use util::{mem, mib, pct, settle, Pct};

pub(crate) const MEM_CAP: usize = 64 << 20; // wasmMemoryMb default (Java: 64)
pub(crate) const CALL_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) struct Args {
    cmd: String,
    engine: String,
    n: usize,
    c: usize,
    secs: f64,
    precompiled: bool,
    fresh: bool,
    mode: String,
    b: usize,
    rate: f64,
    guest: Option<PathBuf>,
    export: String,
}

fn args() -> Args {
    let v: Vec<String> = std::env::args().collect();
    let get = |k: &str| v.iter().position(|a| a == k).map(|i| v[i + 1].clone());
    Args {
        cmd: v.get(1).cloned().unwrap_or_default(),
        engine: v.get(2).cloned().unwrap_or_default(),
        n: get("--n").map(|s| s.parse().unwrap()).unwrap_or(1),
        c: get("--c").map(|s| s.parse().unwrap()).unwrap_or(1),
        secs: get("--secs").map(|s| s.parse().unwrap()).unwrap_or(5.0),
        precompiled: v.iter().any(|a| a == "--precompiled"),
        fresh: v.iter().any(|a| a == "--fresh"),
        mode: get("--mode").unwrap_or_else(|| "spin".into()),
        b: get("--b").map(|s| s.parse().unwrap()).unwrap_or(28),
        rate: get("--rate").map(|s| s.parse().unwrap()).unwrap_or(500.0),
        guest: get("--guest").map(PathBuf::from),
        export: get("--export").unwrap_or_else(|| "echo".into()),
    }
}

pub(crate) fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn fixture() -> Result<Vec<u8>> {
    util::verify_sha256(&root().join("fixtures/fc_test_guest.wasm"))
}

fn kernel_wasm() -> Result<Vec<u8>> {
    Ok(std::fs::read(root().join("fixtures/extism-runtime-endive.wasm"))?)
}

pub(crate) fn scratch() -> PathBuf {
    let d = std::env::var("FC_SCRATCH").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir().join("fnhost-density"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub(crate) fn ctx(i: usize) -> Arc<FnCtx> {
    Arc::new(FnCtx {
        address: format!("bench.f{i:04}"),
        config: [("greeting".to_string(), "hello".to_string())].into(),
        secrets: [("token".to_string(), "s3cret".to_string())].into(),
        policy: EgressPolicy { allow: vec!["127.0.0.1".into(), "localhost".into(), "api.example.com".into()] },
        logs: Default::default(),
    })
}

/// A function loaded on a synchronous engine, (a) or (b).
enum SyncFn {
    Ex(eng_extism::ExFunc),
    Kb(eng_kernel::KFunc),
}

struct SyncHost {
    kb: Option<Arc<eng_kernel::KernelEngine>>,
    ex_cfg: eng_extism::ExCfg,
}

impl SyncHost {
    fn new(engine: &str, slots: u32, timeout: Duration) -> Result<Self> {
        let kb = match engine {
            "kernel" => Some(Arc::new(eng_kernel::KernelEngine::new(Some(&kernel_wasm()?), slots, MEM_CAP)?)),
            "native" => Some(Arc::new(eng_kernel::KernelEngine::new(None, slots, MEM_CAP)?)),
            _ => None,
        };
        let ex_cfg = eng_extism::ExCfg {
            max_pages: Some((MEM_CAP / 65536) as u32),
            timeout,
            cache_dir: None,
            shadow: true,
            reserve_mb: None,
        };
        Ok(Self { kb, ex_cfg })
    }

    fn load(&self, wasm: &[u8], ctx: Arc<FnCtx>) -> Result<SyncFn> {
        Ok(match &self.kb {
            Some(kb) => SyncFn::Kb(kb.load(&kb.compile(wasm)?, ctx, MEM_CAP)?),
            None => SyncFn::Ex(eng_extism::load(wasm, ctx, &self.ex_cfg)?),
        })
    }

    fn call(&self, f: &SyncFn, export: &str, input: &[u8], timeout: Duration, fresh: bool) -> Result<Vec<u8>> {
        match f {
            SyncFn::Ex(f) => eng_extism::pooled_call(f, export, input),
            SyncFn::Kb(f) => {
                let kb = self.kb.as_ref().unwrap();
                if fresh {
                    Ok(kb.fresh_call(f, export, input, timeout)?)
                } else {
                    Ok(kb.pooled_call(f, export, input, timeout)?)
                }
            }
        }
    }
}

pub(crate) fn line(kv: &[(&str, String)]) {
    let s: Vec<String> = kv.iter().map(|(k, v)| format!("{k}={v}")).collect();
    println!("{}", s.join(" "));
    use std::io::Write;
    std::io::stdout().flush().ok();
}

/// Exit without teardown (dropping thousands of modules is O(n^2) on macOS: __deregister_frame).
pub(crate) fn done() -> ! {
    use std::io::Write;
    std::io::stdout().flush().ok();
    std::process::exit(0)
}

pub(crate) fn pct_ms(p: &Pct) -> String {
    format!("p50={:.3}ms,p99={:.3}ms,max={:.3}ms", p.p50 / 1000.0, p.p99 / 1000.0, p.max / 1000.0)
}

// ── density ───────────────────────────────────────────────────────────────────

fn density_sync(a: &Args) -> Result<()> {
    let wasm = match &a.guest {
        Some(p) => std::fs::read(p)?,
        None => fixture()?,
    };
    let (r0, f0) = mem();
    let sync = SyncHost::new(&a.engine, (a.n + 16) as u32, CALL_TIMEOUT)?;
    settle();
    let (r_eng, f_eng) = mem();

    let mut compile = Vec::with_capacity(a.n);
    let mut fns = Vec::with_capacity(a.n);
    let mut precompile_ms = 0.0;
    let mut cwasm_bytes = 0u64;
    let dir = scratch().join(format!("cwasm-{}-{}", a.engine, std::process::id()));
    if a.precompiled {
        match &sync.kb {
            Some(kb) => {
                let t = Instant::now();
                let bytes = kb.precompile(&wasm)?;
                precompile_ms = t.elapsed().as_secs_f64() * 1e3;
                cwasm_bytes = bytes.len() as u64;
                std::fs::create_dir_all(&dir)?;
                for i in 0..a.n {
                    std::fs::write(dir.join(format!("f{i}.cwasm")), &bytes)?;
                }
                let (r_pre, f_pre) = mem();
                let _ = (r_pre, f_pre);
                for i in 0..a.n {
                    let t = Instant::now();
                    let m = kb.deserialize_file(&dir.join(format!("f{i}.cwasm")))?;
                    let f = kb.load(&m, ctx(i), MEM_CAP)?;
                    compile.push(t.elapsed());
                    fns.push(SyncFn::Kb(f));
                }
            }
            None => {
                // extism has no public deserialize; its analogue is wasmtime's on-disk cache.
                let cache = dir.join("cache");
                std::fs::create_dir_all(&cache)?;
                let conf = dir.join("cache.toml");
                std::fs::write(&conf, format!("[cache]\ndirectory = \"{}\"\n", cache.display()))?;
                let mut cfg = sync.ex_cfg.clone();
                cfg.cache_dir = Some(conf);
                let t = Instant::now();
                drop(eng_extism::load(&wasm, ctx(0), &cfg)?); // populate the cache
                precompile_ms = t.elapsed().as_secs_f64() * 1e3;
                for i in 0..a.n {
                    let t = Instant::now();
                    fns.push(SyncFn::Ex(eng_extism::load(&wasm, ctx(i), &cfg)?));
                    compile.push(t.elapsed());
                }
            }
        }
    } else {
        for i in 0..a.n {
            let t = Instant::now();
            fns.push(sync.load(&wasm, ctx(i))?);
            compile.push(t.elapsed());
        }
    }
    settle();
    let (r_idle, f_idle) = mem();

    let input = util::abi_request("bench.echo", "");
    let mut first = Vec::with_capacity(a.n);
    for f in &fns {
        let t = Instant::now();
        let out = sync.call(f, "echo", &input, CALL_TIMEOUT, false)?;
        first.push(t.elapsed());
        if out.is_empty() {
            bail!("empty echo");
        }
    }
    settle();
    let (r_warm, f_warm) = mem();
    // second call (warm instance reused) for reference
    let mut second = Vec::with_capacity(a.n);
    for f in &fns {
        let t = Instant::now();
        sync.call(f, "echo", &input, CALL_TIMEOUT, false)?;
        second.push(t.elapsed());
    }
    let _ = std::fs::remove_dir_all(&dir);
    let n = a.n as f64;
    let cp = pct(&mut compile);
    line(&[
        ("scenario", "density".into()),
        ("engine", a.engine.clone()),
        ("n", a.n.to_string()),
        ("precompiled", a.precompiled.to_string()),
        ("rss_start_mb", format!("{:.1}", mib(r0))),
        ("rss_engine_mb", format!("{:.1}", mib(r_eng))),
        ("rss_idle_mb", format!("{:.1}", mib(r_idle))),
        ("rss_warm_mb", format!("{:.1}", mib(r_warm))),
        ("fp_start_mb", format!("{:.1}", mib(f0))),
        ("fp_engine_mb", format!("{:.1}", mib(f_eng))),
        ("fp_idle_mb", format!("{:.1}", mib(f_idle))),
        ("fp_warm_mb", format!("{:.1}", mib(f_warm))),
        ("idle_rss_per_fn_kb", format!("{:.1}", (r_idle as f64 - r_eng as f64) / n / 1024.0)),
        ("warm_rss_per_fn_kb", format!("{:.1}", (r_warm as f64 - r_eng as f64) / n / 1024.0)),
        ("idle_fp_per_fn_kb", format!("{:.1}", (f_idle as f64 - f_eng as f64) / n / 1024.0)),
        ("warm_fp_per_fn_kb", format!("{:.1}", (f_warm as f64 - f_eng as f64) / n / 1024.0)),
        ("load", pct_ms(&cp)),
        ("load_mean_ms", format!("{:.3}", cp.mean / 1000.0)),
        ("precompile_once_ms", format!("{precompile_ms:.1}")),
        ("cwasm_bytes", cwasm_bytes.to_string()),
        ("first_call", pct_ms(&pct(&mut first))),
        ("second_call", pct_ms(&pct(&mut second))),
    ]);
    done()
}

// ── steady ────────────────────────────────────────────────────────────────────

fn steady_sync(a: &Args) -> Result<()> {
    let wasm = match &a.guest {
        Some(p) => std::fs::read(p)?,
        None => fixture()?,
    };
    let sync = Arc::new(SyncHost::new(&a.engine, (a.c * 2 + 16) as u32, CALL_TIMEOUT)?);
    let f = Arc::new(sync.load(&wasm, ctx(0))?);
    let input = Arc::new(util::abi_request("bench.echo", ""));
    let export = a.export.clone();
    let run = |secs: f64, record: bool| -> (Vec<Duration>, f64) {
        let stop = Arc::new(AtomicBool::new(false));
        let all = Arc::new(Mutex::new(Vec::new()));
        let t0 = Instant::now();
        let hs: Vec<_> = (0..a.c)
            .map(|_| {
                let (sync, f, input, stop, all, export) =
                    (sync.clone(), f.clone(), input.clone(), stop.clone(), all.clone(), export.clone());
                let fresh = a.fresh;
                std::thread::spawn(move || {
                    let mut mine = Vec::with_capacity(100_000);
                    while !stop.load(Ordering::Relaxed) {
                        let t = Instant::now();
                        sync.call(&f, &export, &input, CALL_TIMEOUT, fresh).expect("call");
                        mine.push(t.elapsed());
                    }
                    if record {
                        all.lock().unwrap().extend(mine);
                    }
                })
            })
            .collect();
        std::thread::sleep(Duration::from_secs_f64(secs));
        stop.store(true, Ordering::Relaxed);
        for h in hs {
            h.join().unwrap();
        }
        let el = t0.elapsed().as_secs_f64();
        let v = std::mem::take(&mut *all.lock().unwrap());
        (v, el)
    };
    run(1.0, false); // warm-up
    let (mut v, el) = run(a.secs, true);
    let p = pct(&mut v);
    let (rss, fp) = mem();
    line(&[
        ("scenario", "steady".into()),
        ("engine", a.engine.clone()),
        ("export", a.export.clone()),
        ("c", a.c.to_string()),
        ("fresh", a.fresh.to_string()),
        ("calls", p.n.to_string()),
        ("rps", format!("{:.0}", p.n as f64 / el)),
        ("p50_us", format!("{:.1}", p.p50)),
        ("p90_us", format!("{:.1}", p.p90)),
        ("p99_us", format!("{:.1}", p.p99)),
        ("max_us", format!("{:.1}", p.max)),
        ("rss_mb", format!("{:.1}", mib(rss))),
        ("fp_mb", format!("{:.1}", mib(fp))),
    ]);
    Ok(())
}

// ── noisy neighbour ───────────────────────────────────────────────────────────

/// A at a fixed rate (open loop: latency from the scheduled start, so queueing counts).
fn open_loop<F: Fn() + Send + Sync + 'static>(rate: f64, secs: f64, workers: usize, f: Arc<F>) -> Vec<Duration> {
    let (tx, rx) = std::sync::mpsc::channel::<Instant>();
    let rx = Arc::new(Mutex::new(rx));
    let out = Arc::new(Mutex::new(Vec::new()));
    let hs: Vec<_> = (0..workers)
        .map(|_| {
            let (rx, out, f) = (rx.clone(), out.clone(), f.clone());
            std::thread::spawn(move || loop {
                let next = rx.lock().unwrap().recv();
                let Ok(sched) = next else { break };
                f();
                out.lock().unwrap().push(sched.elapsed());
            })
        })
        .collect();
    let start = Instant::now();
    let total = (rate * secs) as u64;
    for i in 0..total {
        let at = start + Duration::from_secs_f64(i as f64 / rate);
        let now = Instant::now();
        if at > now {
            std::thread::sleep(at - now);
        }
        // Latency counts from the actual dispatch instant (timer slack is not the host's), and
        // still includes any wait for a free worker.
        tx.send(Instant::now()).unwrap();
    }
    drop(tx);
    for h in hs {
        h.join().unwrap();
    }
    let v = std::mem::take(&mut *out.lock().unwrap());
    v
}

fn neighbour_sync(a: &Args) -> Result<()> {
    let wasm = fixture()?;
    let slots = (a.b * 2 + 64) as u32;
    let sync_a = Arc::new(SyncHost::new(&a.engine, slots, CALL_TIMEOUT)?);
    // B's calls time out at 100 ms (spin never returns) — a separate Sync for extism, whose
    // timeout is per compiled plugin; for (b) the same engine is reused.
    let sync_b = if a.engine != "extism" {
        sync_a.clone()
    } else {
        Arc::new(SyncHost::new(&a.engine, slots, Duration::from_millis(100))?)
    };
    let fa = Arc::new(sync_a.load(&wasm, ctx(0))?);
    let fb = Arc::new(sync_b.load(&wasm, ctx(1))?);
    let input = Arc::new(util::abi_request("bench.a", ""));
    let b_input = Arc::new(util::abi_request("bench.b", if a.mode == "alloc" { "mb=16" } else { "" }));
    let call_a = {
        let (s, f, i) = (sync_a.clone(), fa.clone(), input.clone());
        Arc::new(move || {
            s.call(&f, "echo", &i, CALL_TIMEOUT, false).expect("A call");
        })
    };
    // warm-up A
    open_loop(a.rate, 1.0, 16, call_a.clone());
    let mut alone = open_loop(a.rate, a.secs, 16, call_a.clone());

    let stop = Arc::new(AtomicBool::new(false));
    let b_calls = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let hs: Vec<_> = (0..a.b)
        .map(|_| {
            let (s, f, i, stop, n, mode) = (sync_b.clone(), fb.clone(), b_input.clone(), stop.clone(), b_calls.clone(), a.mode.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let _ = s.call(&f, &mode, &i, Duration::from_millis(100), false);
                    n.fetch_add(1, Ordering::Relaxed);
                }
            })
        })
        .collect();
    std::thread::sleep(Duration::from_millis(500));
    let mut busy = open_loop(a.rate, a.secs, 16, call_a);
    stop.store(true, Ordering::Relaxed);
    for h in hs {
        h.join().unwrap();
    }
    let (pa, pb) = (pct(&mut alone), pct(&mut busy));
    line(&[
        ("scenario", "neighbour".into()),
        ("engine", a.engine.clone()),
        ("b_mode", a.mode.clone()),
        ("b_threads", a.b.to_string()),
        ("a_rate", a.rate.to_string()),
        ("b_calls", b_calls.load(Ordering::Relaxed).to_string()),
        ("alone_p50_us", format!("{:.1}", pa.p50)),
        ("alone_p99_us", format!("{:.1}", pa.p99)),
        ("busy_p50_us", format!("{:.1}", pb.p50)),
        ("busy_p99_us", format!("{:.1}", pb.p99)),
        ("p50_degradation_pct", format!("{:+.0}", (pb.p50 / pa.p50 - 1.0) * 100.0)),
        ("p99_degradation_pct", format!("{:+.0}", (pb.p99 / pa.p99 - 1.0) * 100.0)),
    ]);
    Ok(())
}

// ── decision 5: egress, kernel memory, logs, headers ──────────────────────────

fn egress_sync(a: &Args) -> Result<()> {
    let wasm = fixture()?;
    let port = policy::start_test_server();
    let sync = SyncHost::new(&a.engine, 32, Duration::from_millis(1000))?;
    // 16 MiB cap, as Java's theKernelsOwnMemoryIsCappedByWasmMemoryMb.
    let mut sync16 = SyncHost::new(&a.engine, 32, Duration::from_millis(5000))?;
    if a.engine == "kernel" || a.engine == "native" {
        let kw = kernel_wasm()?;
        let kw = (a.engine == "kernel").then_some(kw.as_slice());
        sync16.kb = Some(Arc::new(eng_kernel::KernelEngine::new(kw, 32, 16 << 20)?));
    } else {
        sync16.ex_cfg.max_pages = Some(256);
        if a.mode == "reserve" {
            sync16.ex_cfg.max_pages = None;
            sync16.ex_cfg.reserve_mb = Some(16);
        }
    }
    let c = ctx(0);
    let f = sync.load(&wasm, c.clone())?;
    let f16 = match &sync16.kb {
        Some(kb) => SyncFn::Kb(kb.load(&kb.compile(&wasm)?, c.clone(), 16 << 20)?),
        None => sync16.load(&wasm, c.clone())?,
    };
    let call = |f: &SyncFn, s: &SyncHost, export: &str, q: &str| -> String {
        match s.call(f, export, &util::abi_request("bench.egress", q), Duration::from_millis(1000), false) {
            Ok(o) => {
                let v: serde_json::Value = serde_json::from_slice(&o).unwrap_or_default();
                v["body"].as_str().map(|s| s.to_string()).unwrap_or_else(|| String::from_utf8_lossy(&o).to_string())
            }
            Err(e) => format!("TRAP/ERROR: {}", format!("{e:#}").replace('\n', " ")),
        }
    };
    let cases = [
        ("allowed loopback, repeated headers", format!("url=http://127.0.0.1:{port}/ok")),
        ("redirect not followed", format!("url=http://127.0.0.1:{port}/redirect")),
        ("http to non-loopback denied", "url=http://api.example.com/x".to_string()),
        ("host not on allowlist denied", "url=https://evil.example.org/x".to_string()),
        ("deadline caps the call (1 s call, 2 s upstream)", format!("url=http://127.0.0.1:{port}/slow")),
    ];
    for (name, q) in cases {
        let t = Instant::now();
        let r = call(&f, &sync, "http", &q);
        println!("egress engine={} case=\"{name}\" ms={} result={r}", a.engine, t.elapsed().as_millis());
    }
    println!(
        "egress engine={} case=\"kalloc 64 MiB under a 16 MiB cap\" result={}",
        a.engine,
        call(&f16, &sync16, "kalloc", "mb=64")
    );
    println!("egress engine={} case=\"alloc 32 MiB under a 16 MiB cap\" result={}", a.engine, call(&f16, &sync16, "alloc", "mb=32"));
    println!("egress engine={} case=\"next call after cap failure\" result={}", a.engine, call(&f16, &sync16, "alloc", "mb=1"));
    c.logs.take();
    let r = call(&f, &sync, "log", "msg=hi");
    println!("egress engine={} case=\"log\" result={r} captured={:?}", a.engine, c.logs.take());
    println!("egress engine={} case=\"config declared\" result={}", a.engine, call(&f, &sync, "config", "key=greeting"));
    println!("egress engine={} case=\"secret declared\" result={}", a.engine, call(&f, &sync, "secret", "key=token"));
    println!("egress engine={} case=\"secret missing\" result={}", a.engine, call(&f, &sync, "secret", "key=nope"));
    Ok(())
}

fn inspect() -> Result<()> {
    let e = wasmtime::Engine::default();
    for (name, bytes) in [("guest", fixture()?), ("kernel", kernel_wasm()?)] {
        let m = wasmtime::Module::new(&e, &bytes)?;
        for i in m.imports() {
            println!("{name} import {}::{} {:?}", i.module(), i.name(), i.ty());
        }
        for x in m.exports() {
            println!("{name} export {} {:?}", x.name(), x.ty());
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let a = args();
    match (a.cmd.as_str(), a.engine.as_str()) {
        ("inspect", _) => inspect(),
        ("density", "extism" | "kernel" | "native") => density_sync(&a),
        ("steady", "extism" | "kernel" | "native") => steady_sync(&a),
        ("neighbour", "extism" | "kernel" | "native") => neighbour_sync(&a),
        ("egress", "extism" | "kernel" | "native") => egress_sync(&a),
        (cmd, "component") => eng_comp::run(cmd, &a),
        _ => bail!("usage: fnhost-density <inspect|density|steady|neighbour|egress> <extism|kernel|component> [--n N] [--c C] [--secs S] [--precompiled] [--fresh] [--mode spin|alloc] [--b B] [--rate R]"),
    }
    .context("run")?;
    // Skip teardown: dropping thousands of stores and the pooling allocator's reservations
    // takes minutes and measures nothing.
    use std::io::Write;
    std::io::stdout().flush().ok();
    std::process::exit(0)
}

#[allow(dead_code)]
fn exists(p: &Path) -> bool {
    p.exists()
}
