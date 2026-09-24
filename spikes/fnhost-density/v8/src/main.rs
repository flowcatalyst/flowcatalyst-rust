//! (e) Baseline: V8 isolates through deno_core, one isolate per function (the Supabase
//! edge-runtime model), running a trivial JSON-transform handler. Measures memory per isolate
//! (with a startup snapshot that already contains the handler), first call and steady call.
//!
//!   fnhost-v8 density --n N [--no-snapshot]
//!   fnhost-v8 steady --c C --secs S

use deno_core::{v8, JsRuntime, JsRuntimeForSnapshot, RuntimeOptions};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const HANDLER: &str = r#"
globalThis.handler = (input) => {
  const req = JSON.parse(input);
  return JSON.stringify({ status: 200, headers: { "content-type": ["application/json"] },
                          body: JSON.stringify({ method: req.method, path: req.path, n: req.n, echo: req }) });
};
"#;

const INPUT: &str = r#"{"address":"bench.echo","version":1,"invocationId":"inv_0HZXEQ5Y8JY5Z","method":"POST","path":"/x","originalHost":"fn.example.test","originalPath":"/functions/bench.echo/x","pathParams":{},"query":{},"headers":{"content-type":["application/json"],"x-request-id":["r-1"]},"bodyBase64":"eyJoZWxsbyI6IndvcmxkIiwibiI6NDJ9","remoteAddress":"127.0.0.1","caller":{"kind":"principal","id":"prn_0HZXEQ5Y8JY5Z","type":"SERVICE","tier":"CLIENT","clients":["clt_0HZXEQ5Y8JY5Z"],"roles":["function-publisher"],"applications":[],"allApplications":false,"permissions":["platform:function:version:invoke"]},"n":42}"#;

fn mem() -> (u64, u64) {
    unsafe {
        let mut info: libc::rusage_info_v2 = std::mem::zeroed();
        libc::proc_pid_rusage(libc::getpid(), libc::RUSAGE_INFO_V2, &mut info as *mut _ as *mut libc::rusage_info_t);
        (info.ri_resident_size, info.ri_phys_footprint)
    }
}

fn mib(b: u64) -> f64 {
    b as f64 / 1048576.0
}

fn snapshot() -> &'static [u8] {
    let mut rt = JsRuntimeForSnapshot::new(RuntimeOptions::default());
    rt.execute_script("[handler]", HANDLER.to_string()).expect("handler");
    Box::leak(rt.snapshot())
}

struct Fn {
    rt: JsRuntime,
    handler: v8::Global<v8::Function>,
}

fn new_fn(snap: Option<&'static [u8]>) -> Fn {
    let mut rt = JsRuntime::new(RuntimeOptions { startup_snapshot: snap, ..Default::default() });
    if snap.is_none() {
        rt.execute_script("[handler]", HANDLER.to_string()).expect("handler");
    }
    let g = rt.execute_script("[get]", "globalThis.handler".to_string()).expect("get");
    let handler = {
        deno_core::scope!(scope, &mut rt);
        let v = v8::Local::new(scope, g);
        let f = v8::Local::<v8::Function>::try_from(v).expect("function");
        v8::Global::new(scope, f)
    };
    Fn { rt, handler }
}

fn call(f: &mut Fn, input: &str) -> String {
    let handler = &f.handler;
    deno_core::scope!(scope, &mut f.rt);
    let func = v8::Local::new(scope, handler);
    let arg = v8::String::new(scope, input).unwrap();
    let undef = v8::undefined(scope);
    let r = func.call(scope, undef.into(), &[arg.into()]).expect("call");
    r.to_rust_string_lossy(scope)
}

fn pct(v: &mut [Duration]) -> (f64, f64, f64) {
    v.sort_unstable();
    let at = |q: f64| v[((v.len() as f64 - 1.0) * q).round() as usize].as_secs_f64() * 1e6;
    (at(0.5), at(0.99), v.last().unwrap().as_secs_f64() * 1e6)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |k: &str| args.iter().position(|a| a == k).map(|i| args[i + 1].clone());
    let cmd = args.get(1).cloned().unwrap_or_default();
    let n: usize = get("--n").map(|s| s.parse().unwrap()).unwrap_or(1);
    let c: usize = get("--c").map(|s| s.parse().unwrap()).unwrap_or(1);
    let secs: f64 = get("--secs").map(|s| s.parse().unwrap()).unwrap_or(5.0);
    let use_snap = !args.iter().any(|a| a == "--no-snapshot");
    JsRuntime::init_platform(None);
    match cmd.as_str() {
        "density" => {
            let (r0, f0) = mem();
            let t = Instant::now();
            let snap = use_snap.then(snapshot);
            let snap_ms = t.elapsed().as_secs_f64() * 1e3;
            let (r_eng, f_eng) = mem();
            let mut fns = Vec::with_capacity(n);
            let mut create = Vec::with_capacity(n);
            let mut first = Vec::with_capacity(n);
            for _ in 0..n {
                let t = Instant::now();
                let mut f = new_fn(snap);
                create.push(t.elapsed());
                let t2 = Instant::now();
                let out = call(&mut f, INPUT);
                first.push(t2.elapsed());
                assert!(out.contains("\"status\":200"));
                fns.push(f);
            }
            std::thread::sleep(Duration::from_millis(300));
            let (r1, f1) = mem();
            let mut cold: Vec<Duration> = create.iter().zip(&first).map(|(a, b)| *a + *b).collect();
            let (cp50, cp99, _) = pct(&mut create);
            let (fp50, fp99, _) = pct(&mut first);
            let (k50, k99, kmax) = pct(&mut cold);
            println!(
                "scenario=density engine=v8 n={n} snapshot={use_snap} snapshot_build_ms={snap_ms:.1} rss_start_mb={:.1} rss_engine_mb={:.1} rss_loaded_mb={:.1} fp_engine_mb={:.1} fp_loaded_mb={:.1} rss_per_fn_kb={:.1} fp_per_fn_kb={:.1} create_p50_ms={:.3} create_p99_ms={:.3} first_call_p50_ms={:.3} first_call_p99_ms={:.3} cold_p50_ms={:.3} cold_p99_ms={:.3} cold_max_ms={:.3}",
                mib(r0), mib(r_eng), mib(r1), mib(f_eng), mib(f1),
                (r1 as f64 - r_eng as f64) / n as f64 / 1024.0,
                (f1 as f64 - f_eng as f64) / n as f64 / 1024.0,
                cp50 / 1e3, cp99 / 1e3, fp50 / 1e3, fp99 / 1e3, k50 / 1e3, k99 / 1e3, kmax / 1e3
            );
            let _ = f0;
            std::process::exit(0);
        }
        "steady" => {
            let snap = snapshot();
            let stop = Arc::new(AtomicBool::new(false));
            let all = Arc::new(Mutex::new(Vec::new()));
            let started = Arc::new(std::sync::Barrier::new(c + 1));
            let hs: Vec<_> = (0..c)
                .map(|_| {
                    let (stop, all, started) = (stop.clone(), all.clone(), started.clone());
                    std::thread::spawn(move || {
                        let mut f = new_fn(Some(snap));
                        let warm = Instant::now();
                        while warm.elapsed() < Duration::from_secs(1) {
                            call(&mut f, INPUT);
                        }
                        started.wait();
                        let mut mine = Vec::with_capacity(200_000);
                        while !stop.load(Ordering::Relaxed) {
                            let t = Instant::now();
                            call(&mut f, INPUT);
                            mine.push(t.elapsed());
                        }
                        all.lock().unwrap().extend(mine);
                        std::mem::forget(f); // isolates must not be dropped out of order across threads
                    })
                })
                .collect();
            started.wait();
            let t0 = Instant::now();
            std::thread::sleep(Duration::from_secs_f64(secs));
            stop.store(true, Ordering::Relaxed);
            for h in hs {
                h.join().unwrap();
            }
            let el = t0.elapsed().as_secs_f64();
            let mut v = std::mem::take(&mut *all.lock().unwrap());
            let calls = v.len();
            let (p50, p99, max) = pct(&mut v);
            let (rss, fp) = mem();
            println!(
                "scenario=steady engine=v8 c={c} calls={calls} rps={:.0} p50_us={p50:.1} p99_us={p99:.1} max_us={max:.1} rss_mb={:.1} fp_mb={:.1}",
                calls as f64 / el,
                mib(rss),
                mib(fp)
            );
            std::process::exit(0);
        }
        _ => eprintln!("usage: fnhost-v8 density --n N [--no-snapshot] | steady --c C --secs S"),
    }
}
