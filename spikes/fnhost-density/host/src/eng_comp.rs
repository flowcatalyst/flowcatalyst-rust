//! Variant (c): WASI 0.2 components + `wasi:http/proxy` on wasmtime 49 (async, tokio).
//! One `Engine` (pooling allocator, epoch interruption with a 1 ms async yield), one
//! `Component` + `ProxyPre` per function version, `StoreLimits` per store. Outbound HTTP goes
//! through `WasiHttpHooks::send_request`, where the host policy runs: https only except loopback,
//! the function's allowlist, no redirects (hyper never follows them), deadline-capped timeout; a
//! denial is the typed `wasi:http` `error-code.HTTP-request-denied`. The FlowCatalyst host
//! interface (`flowcatalyst:function/host`, wit/flowcatalyst-function.wit) is bound with `bindgen!`.

use crate::policy::{self, Decision, FnCtx};
use crate::util::{self, mem, mib, pct};
use crate::wasi_out::LogOut;
use crate::{ctx, line, pct_ms, Args, MEM_CAP};
use anyhow::{anyhow, bail, Result};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wasmtime::component::{Component, HasSelf, Linker, ResourceTable};
use wasmtime::{Engine, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};
use wasmtime_wasi_http::p2::bindings::http::types::Scheme;
use wasmtime_wasi_http::p2::bindings::{Proxy, ProxyPre};
use wasmtime_wasi_http::{RequestOptions, WasiHttpCtx, WasiHttpCtxView, WasiHttpHooks, WasiHttpView};

wasmtime::component::bindgen!({ path: "../wit", world: "guest-imports" });
use flowcatalyst::function::host as fchost;

pub struct FcHooks {
    policy: policy::EgressPolicy,
    deadline: Instant,
}

type SendFut = Box<
    dyn Future<
            Output = wasmtime_wasi_http::Result<(
                http::Response<wasmtime_wasi_http::WasiBody>,
                Box<dyn Future<Output = wasmtime_wasi_http::Result<()>> + Send>,
            )>,
        > + Send,
>;

impl WasiHttpHooks for FcHooks {
    fn send_request(
        &mut self,
        request: http::Request<wasmtime_wasi_http::WasiBody>,
        options: Option<RequestOptions>,
        fut: Box<dyn Future<Output = wasmtime_wasi_http::Result<()>> + Send>,
    ) -> SendFut {
        let _ = fut;
        let scheme = request.uri().scheme_str().unwrap_or("").to_string();
        let host = request.uri().host().unwrap_or("").to_string();
        match self.policy.check(&scheme, &host, None, self.deadline) {
            Decision::Deny(_why) => Box::new(async { Err(wasmtime_wasi_http::Error::HttpRequestDenied) }),
            Decision::Allow { timeout } => {
                let mut o = options.unwrap_or_default();
                let cap = |t: Option<Duration>| Some(t.map_or(timeout, |t| t.min(timeout)));
                o.connect_timeout = cap(o.connect_timeout);
                o.first_byte_timeout = cap(o.first_byte_timeout);
                o.between_bytes_timeout = cap(o.between_bytes_timeout);
                Box::new(async move {
                    // hyper does not follow redirects: the guest sees the 3xx.
                    let sent = tokio::time::timeout(timeout, wasmtime_wasi_http::default_send_request(request, Some(o)));
                    let (res, io) = match sent.await {
                        Ok(r) => r?,
                        Err(_) => return Err(wasmtime_wasi_http::Error::HttpResponseTimeout),
                    };
                    Ok((res.map(|b| b.boxed_unsync()), Box::new(io) as Box<dyn Future<Output = _> + Send>))
                })
            }
        }
    }
}

pub struct CState {
    wasi: WasiCtx,
    http: WasiHttpCtx,
    table: ResourceTable,
    limits: StoreLimits,
    hooks: FcHooks,
    ctx: Arc<FnCtx>,
    stdout: LogOut,
    stderr: LogOut,
}

impl WasiView for CState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView { ctx: &mut self.wasi, table: &mut self.table }
    }
}

impl WasiHttpView for CState {
    fn http(&mut self) -> WasiHttpCtxView<'_> {
        WasiHttpCtxView { ctx: &mut self.http, table: &mut self.table, hooks: &mut self.hooks }
    }
}

impl fchost::Host for CState {
    fn log(&mut self, level: fchost::Level, message: String) {
        let l = match level {
            fchost::Level::Trace => "TRACE",
            fchost::Level::Debug => "DEBUG",
            fchost::Level::Info => "INFO",
            fchost::Level::Warn => "WARN",
            fchost::Level::Error => "ERROR",
        };
        self.ctx.logs.push(l, &message);
    }
    fn config_get(&mut self, key: String) -> Option<String> {
        self.ctx.config.get(&key).cloned()
    }
    fn secret_get(&mut self, key: String) -> Option<String> {
        self.ctx.secrets.get(&key).cloned()
    }
    fn emit_event(&mut self, event: fchost::OutboundEvent) -> Result<(), fchost::EmitError> {
        if event.dedup_id.is_empty() {
            return Err(fchost::EmitError::InvalidEvent("dedup-id is required".into()));
        }
        Ok(())
    }
}

/// The per-store memory cap for components: `FC_MEM_MB`, else the 64 MiB default.
pub fn comp_mem_cap() -> usize {
    std::env::var("FC_MEM_MB").ok().and_then(|v| v.parse::<usize>().ok()).map(|mb| mb << 20).unwrap_or(MEM_CAP)
}

pub struct CompEngine {
    pub engine: Engine,
    linker: Linker<CState>,
}

pub struct CFunc {
    pre: ProxyPre<CState>,
    ctx: Arc<FnCtx>,
    mem_cap: usize,
    pool: Mutex<Vec<(Store<CState>, Proxy)>>,
}

impl CompEngine {
    pub fn new(slots: u32) -> Result<Self> {
        let mut cfg = crate::eng_kernel::pooled_config(slots, 8, 2, comp_mem_cap());
        let engine = Engine::new(&cfg)?;
        crate::eng_kernel::start_ticker(&engine);
        let mut linker = Linker::new(&engine);
        wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
        wasmtime_wasi_http::p2::add_only_http_to_linker_async(&mut linker)?;
        fchost::add_to_linker::<CState, HasSelf<CState>>(&mut linker, |s| s)?;
        Ok(Self { engine, linker })
    }

    pub fn load(&self, c: &Component, ctx: Arc<FnCtx>) -> Result<CFunc> {
        let pre = ProxyPre::new(self.linker.instantiate_pre(c)?)?;
        Ok(CFunc { pre, ctx, mem_cap: comp_mem_cap(), pool: Mutex::new(Vec::new()) })
    }

    pub async fn instantiate(&self, f: &CFunc) -> Result<(Store<CState>, Proxy)> {
        let stdout = LogOut::new("INFO", f.ctx.logs.clone());
        let stderr = LogOut::new("WARN", f.ctx.logs.clone());
        let wasi = WasiCtxBuilder::new().stdout(stdout.clone()).stderr(stderr.clone()).build();
        let state = CState {
            wasi,
            http: WasiHttpCtx::new(),
            table: ResourceTable::new(),
            limits: StoreLimitsBuilder::new().memory_size(f.mem_cap).build(),
            hooks: FcHooks { policy: f.ctx.policy.clone(), deadline: Instant::now() },
            ctx: f.ctx.clone(),
            stdout,
            stderr,
        };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|s| &mut s.limits);
        if std::env::var_os("FC_NO_YIELD").is_some() {
            store.set_epoch_deadline(u64::MAX / 2);
        } else {
            store.set_epoch_deadline(1);
            store.epoch_deadline_async_yield_and_update(1);
        }
        let proxy = f.pre.instantiate_async(&mut store).await?;
        Ok((store, proxy))
    }

    /// One request. Pooled: reuse an idle instance (discarded on failure or deadline).
    /// Fresh: instance-per-request, the wasi:http idiom.
    pub async fn call(&self, f: &CFunc, pq: &str, body: Vec<u8>, timeout: Duration, fresh: bool) -> Result<(u16, Vec<u8>)> {
        let idle = if fresh { None } else { f.pool.lock().unwrap().pop() };
        let (mut store, proxy) = match idle {
            Some(x) => x,
            None => self.instantiate(f).await?,
        };
        store.data_mut().hooks.deadline = Instant::now() + timeout;
        let req = hyper::Request::builder()
            .method(if body.is_empty() { "GET" } else { "POST" })
            .uri(pq)
            .header("host", "fn.example.test")
            .header("content-type", "application/json")
            .header("x-request-id", "r-1")
            .body(Full::new(Bytes::from(body)).map_err(|e: std::convert::Infallible| -> wasmtime_wasi_http::Error { match e {} }))?;
        let (tx, rx) = tokio::sync::oneshot::channel();
        let req = store.data_mut().http().new_incoming_request(Scheme::Http, req)?;
        let out = store.data_mut().http().new_response_outparam(tx)?;
        // The `wasmtime serve` shape: the handler owns the store in its own task and the caller
        // awaits the response-outparam. (Polling handler and response in one task with
        // `try_join!` worked for the Rust guest but broke StarlingMonkey's event loop.)
        let task = tokio::spawn(async move {
            let r = proxy.wasi_http_incoming_handler().call_handle(&mut store, req, out).await;
            store.data().stdout.flush_line();
            store.data().stderr.flush_line();
            match r {
                // Fresh: drop the store now. StarlingMonkey (as built here) ends the response body
                // only when its store goes away, as `wasmtime serve` does per request.
                Ok(()) if fresh => {
                    drop(store);
                    Ok(None)
                }
                Ok(()) => Ok(Some((store, proxy))),
                Err(e) => {
                    drop(store); // drops the outparam sender: the caller sees "no response"
                    Err(e)
                }
            }
        });
        let abort = task.abort_handle();
        let work = async {
            let resp = rx.await.map_err(|_| anyhow!("guest set no response"))?.map_err(|e| anyhow!("{e:?}"))?;
            let status = resp.status().as_u16();
            let bytes = resp.into_body().collect().await.map_err(|e| anyhow!("{e:?}"))?.to_bytes();
            Ok::<_, anyhow::Error>((status, bytes.to_vec()))
        };
        let r = match tokio::time::timeout(timeout, work).await {
            Ok(Ok(resp)) => match tokio::time::timeout(timeout, task).await {
                Ok(Ok(Ok(kept))) => {
                    if let Some(k) = kept {
                        f.pool.lock().unwrap().push(k);
                    }
                    Ok(resp)
                }
                Ok(Ok(Err(e))) => Err(anyhow!("{e:?}")),
                Ok(Err(e)) => Err(anyhow!("{e:?}")),
                Err(_) => {
                    abort.abort();
                    Err(anyhow!("deadline"))
                }
            },
            Ok(Err(e)) => {
                // No response: report the handler's own error if it has one.
                abort.abort();
                match task.await {
                    Ok(Err(te)) => Err(anyhow!("{te:?}")),
                    _ => Err(e),
                }
            }
            Err(_) => {
                abort.abort();
                Err(anyhow!("deadline"))
            }
        };
        r
    }
}

fn guest_path(a: &Args) -> PathBuf {
    a.guest.clone().unwrap_or_else(|| {
        crate::root().join("guests/comp-echo/target/wasm32-wasip2/release/comp_echo.wasm")
    })
}

const ECHO_BODY: &[u8] = br#"{"hello":"world","n":42}"#;
fn echo_body() -> Vec<u8> {
    if std::env::var_os("FC_EMPTY_BODY").is_some() { Vec::new() } else { ECHO_BODY.to_vec() }
}

pub fn run(cmd: &str, a: &Args) -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    rt.block_on(async {
        match cmd {
            "density" => density(a).await,
            "steady" => steady(a).await,
            "neighbour" => neighbour(a).await,
            "egress" => egress(a).await,
            _ => bail!("unknown command {cmd}"),
        }
    })
}

async fn density(a: &Args) -> Result<()> {
    let wasm = std::fs::read(guest_path(a))?;
    let (r0, f0) = mem();
    let ce = CompEngine::new((a.n + 16) as u32)?;
    util::settle();
    let (r_eng, f_eng) = mem();
    let mut compile = Vec::with_capacity(a.n);
    let mut fns = Vec::with_capacity(a.n);
    let (mut precompile_ms, mut cwasm_bytes) = (0.0, 0u64);
    let dir = crate::scratch().join(format!("cwasm-component-{}", std::process::id()));
    if a.precompiled {
        let t = Instant::now();
        let bytes = ce.engine.precompile_component(&wasm)?;
        precompile_ms = t.elapsed().as_secs_f64() * 1e3;
        cwasm_bytes = bytes.len() as u64;
        std::fs::create_dir_all(&dir)?;
        for i in 0..a.n {
            std::fs::write(dir.join(format!("f{i}.cwasm")), &bytes)?;
        }
        for i in 0..a.n {
            let t = Instant::now();
            let c = unsafe { Component::deserialize_file(&ce.engine, dir.join(format!("f{i}.cwasm")))? };
            fns.push(ce.load(&c, ctx(i))?);
            compile.push(t.elapsed());
        }
    } else {
        for i in 0..a.n {
            let t = Instant::now();
            let c = Component::new(&ce.engine, &wasm)?;
            fns.push(ce.load(&c, ctx(i))?);
            compile.push(t.elapsed());
        }
    }
    util::settle();
    let (r_idle, f_idle) = mem();
    let path = if a.export == "echo" { "/echo".to_string() } else { a.export.clone() };
    let mut first = Vec::with_capacity(a.n);
    for f in &fns {
        let t = Instant::now();
        let (status, _) = ce.call(f, &path, echo_body(), crate::CALL_TIMEOUT, a.fresh).await?;
        first.push(t.elapsed());
        if status != 200 {
            bail!("status {status}");
        }
    }
    util::settle();
    let (r_warm, f_warm) = mem();
    let mut second = Vec::with_capacity(a.n);
    for f in &fns {
        let t = Instant::now();
        ce.call(f, &path, echo_body(), crate::CALL_TIMEOUT, a.fresh).await?;
        second.push(t.elapsed());
    }
    let _ = std::fs::remove_dir_all(&dir);
    let n = a.n as f64;
    let cp = pct(&mut compile);
    line(&[
        ("scenario", "density".into()),
        ("engine", "component".into()),
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
    crate::done()
}

async fn steady(a: &Args) -> Result<()> {
    let wasm = std::fs::read(guest_path(a))?;
    let ce = Arc::new(CompEngine::new((a.c * 2 + 16) as u32)?);
    let c = Component::new(&ce.engine, &wasm)?;
    let f = Arc::new(ce.load(&c, ctx(0))?);
    let path = if a.export == "echo" { "/echo".to_string() } else { a.export.clone() };
    let run = |secs: f64| {
        let (ce, f, path) = (ce.clone(), f.clone(), path.clone());
        let (c, fresh) = (a.c, a.fresh);
        async move {
            let stop = Arc::new(AtomicBool::new(false));
            let t0 = Instant::now();
            let hs: Vec<_> = (0..c)
                .map(|_| {
                    let (ce, f, path, stop) = (ce.clone(), f.clone(), path.clone(), stop.clone());
                    tokio::spawn(async move {
                        let mut mine = Vec::with_capacity(100_000);
                        while !stop.load(Ordering::Relaxed) {
                            let t = Instant::now();
                            ce.call(&f, &path, echo_body(), crate::CALL_TIMEOUT, fresh).await.expect("call");
                            mine.push(t.elapsed());
                        }
                        mine
                    })
                })
                .collect();
            tokio::time::sleep(Duration::from_secs_f64(secs)).await;
            stop.store(true, Ordering::Relaxed);
            let mut all = Vec::new();
            for h in hs {
                all.extend(h.await.unwrap());
            }
            (all, t0.elapsed().as_secs_f64())
        }
    };
    run(1.0).await;
    let (mut v, el) = run(a.secs).await;
    let p = pct(&mut v);
    let (rss, fp) = mem();
    line(&[
        ("scenario", "steady".into()),
        ("engine", "component".into()),
        ("export", path),
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

async fn open_loop(ce: Arc<CompEngine>, f: Arc<CFunc>, rate: f64, secs: f64) -> Vec<Duration> {
    let start = tokio::time::Instant::now();
    let total = (rate * secs) as u64;
    let mut hs = Vec::with_capacity(total as usize);
    for i in 0..total {
        let at = start + Duration::from_secs_f64(i as f64 / rate);
        tokio::time::sleep_until(at).await;
        let (ce, f) = (ce.clone(), f.clone());
        let sent = Instant::now();
        hs.push(tokio::spawn(async move {
            ce.call(&f, "/echo", echo_body(), crate::CALL_TIMEOUT, false).await.expect("A call");
            sent.elapsed()
        }));
    }
    let mut v = Vec::with_capacity(hs.len());
    for h in hs {
        v.push(h.await.unwrap());
    }
    v
}

async fn neighbour(a: &Args) -> Result<()> {
    let wasm = std::fs::read(guest_path(a))?;
    let ce = Arc::new(CompEngine::new((a.b * 2 + 256) as u32)?);
    let c = Component::new(&ce.engine, &wasm)?;
    let fa = Arc::new(ce.load(&c, ctx(0))?);
    let fb = Arc::new(ce.load(&c, ctx(1))?);
    open_loop(ce.clone(), fa.clone(), a.rate, 1.0).await;
    let mut alone = open_loop(ce.clone(), fa.clone(), a.rate, a.secs).await;
    let stop = Arc::new(AtomicBool::new(false));
    let b_calls = Arc::new(AtomicU64::new(0));
    let b_path = if a.mode == "alloc" { "/alloc?mb=16" } else { "/spin" };
    let hs: Vec<_> = (0..a.b)
        .map(|_| {
            let (ce, fb, stop, n) = (ce.clone(), fb.clone(), stop.clone(), b_calls.clone());
            tokio::spawn(async move {
                while !stop.load(Ordering::Relaxed) {
                    let _ = ce.call(&fb, b_path, Vec::new(), Duration::from_millis(100), false).await;
                    n.fetch_add(1, Ordering::Relaxed);
                }
            })
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut busy = open_loop(ce.clone(), fa.clone(), a.rate, a.secs).await;
    stop.store(true, Ordering::Relaxed);
    for h in hs {
        let _ = h.await;
    }
    let (pa, pb) = (pct(&mut alone), pct(&mut busy));
    line(&[
        ("scenario", "neighbour".into()),
        ("engine", "component".into()),
        ("b_mode", a.mode.clone()),
        ("b_tasks", a.b.to_string()),
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

async fn egress(a: &Args) -> Result<()> {
    let wasm = std::fs::read(guest_path(a))?;
    let port = policy::start_test_server();
    let ce = CompEngine::new(64)?;
    let c = Component::new(&ce.engine, &wasm)?;
    let cx = ctx(0);
    let f = ce.load(&c, cx.clone())?;
    let call = |pq: String, ms: u64| {
        let (ce, f) = (&ce, &f);
        async move {
            match ce.call(f, &pq, Vec::new(), Duration::from_millis(ms), false).await {
                Ok((s, b)) => format!("{s} {}", String::from_utf8_lossy(&b)),
                Err(e) => format!("ERROR: {}", format!("{e:#}").replace('\n', " ")),
            }
        }
    };
    let cases = [
        ("allowed loopback, repeated headers", format!("/http?url=http://127.0.0.1:{port}/ok")),
        ("redirect not followed", format!("/http?url=http://127.0.0.1:{port}/redirect")),
        ("http to non-loopback denied", "/http?url=http://api.example.com/x".to_string()),
        ("host not on allowlist denied", "/http?url=https://evil.example.org/x".to_string()),
        ("deadline caps the call (1 s call, 2 s upstream)", format!("/http?url=http://127.0.0.1:{port}/slow")),
    ];
    for (name, pq) in cases {
        let t = Instant::now();
        let r = call(pq, 1000).await;
        println!("egress engine=component case=\"{name}\" ms={} result={r}", t.elapsed().as_millis());
    }
    println!("egress engine=component case=\"alloc 96 MiB under the 64 MiB cap\" result={}", call("/alloc?mb=96".into(), 5000).await);
    println!("egress engine=component case=\"next call after cap failure\" result={}", call("/alloc?mb=1".into(), 5000).await);
    let t = Instant::now();
    println!(
        "egress engine=component case=\"spin stopped at a 200 ms deadline\" result={} ms={}",
        call("/spin".into(), 200).await,
        t.elapsed().as_millis()
    );
    println!("egress engine=component case=\"next call after deadline\" result={}", call("/spin?spin=false".into(), 1000).await);
    cx.logs.take();
    let r = call("/log?msg=hi".into(), 1000).await;
    println!("egress engine=component case=\"log\" result={r} captured={:?}", cx.logs.take());
    println!("egress engine=component case=\"config declared (typed import)\" result={}", call("/config?key=greeting".into(), 1000).await);
    println!("egress engine=component case=\"config undeclared\" result={}", call("/config?key=nope".into(), 1000).await);
    Ok(())
}
