//! Variant (a): the `extism` crate (1.30.0), used through its public API only.
//!
//! One `CompiledPlugin` per function version (each builds its OWN wasmtime `Engine` and compiles
//! the kernel again — that is how the crate works); one `Plugin` per concurrent call.
//! The FlowCatalyst `extism:host/user` functions are ordinary host functions. For decision 5
//! the spike SHADOWS the built-ins `http_request` / `http_status_code` / `http_headers` and
//! `log_*` / `get_log_level` by registering host functions in the `extism:host/env` namespace
//! (the crate's linker has `allow_shadowing(true)` and links user functions after its own).

use crate::policy::{self, FnCtx};
use anyhow::Result;
use extism::{CompiledPlugin, CurrentPlugin, Function, Manifest, Plugin, PluginBuilder, UserData, Val, ValType, Wasm, PTR};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const ENV: &str = "extism:host/env";

#[derive(Clone)]
pub struct ExCfg {
    pub max_pages: Option<u32>,
    pub timeout: Duration,
    /// None = wasmtime cache disabled (a true compile); Some(dir) = extism's wasmtime cache.
    pub cache_dir: Option<PathBuf>,
    /// Shadow the built-in HTTP and log functions with the host's own (decision 5).
    pub shadow: bool,
    /// Optional wasmtime config override (e.g. a per-memory reservation cap).
    pub reserve_mb: Option<u64>,
}

pub struct ExFunc {
    pub compiled: CompiledPlugin,
    pub pool: Mutex<Vec<Plugin>>,
}

/// Per-plugin HTTP state for the shadowed functions, keyed by plugin id (host functions are
/// shared by every plugin made from one CompiledPlugin).
#[derive(Default)]
struct HttpState {
    last: Mutex<HashMap<uuid_key::Key, (i32, Vec<(String, String)>)>>,
}

mod uuid_key {
    pub type Key = u128;
}

fn read(p: &mut CurrentPlugin, v: &Val) -> Result<Vec<u8>> {
    match p.memory_from_val(v) {
        Some(h) if h.offset() != 0 => Ok(p.memory_bytes(h)?.to_vec()),
        _ => Ok(Vec::new()),
    }
}

fn free(p: &mut CurrentPlugin, v: &Val) -> Result<()> {
    if let Some(h) = p.memory_from_val(v) {
        if h.offset() != 0 {
            p.memory_free(h)?;
        }
    }
    Ok(())
}

fn write(p: &mut CurrentPlugin, bytes: &[u8]) -> Result<Val> {
    let h = p.memory_new(bytes)?;
    Ok(p.memory_to_val(h))
}

fn functions(ctx: Arc<FnCtx>, shadow: bool) -> Vec<Function> {
    let mut fns = vec![
        Function::new("fc_secret_get", [PTR], [PTR], UserData::new(ctx.clone()), |p, i, o, ud| {
            let key = String::from_utf8_lossy(&read(p, &i[0])?).to_string();
            let ctx = ud.get()?;
            let v = ctx.lock().unwrap().secrets.get(&key).cloned().unwrap_or_default();
            o[0] = if v.is_empty() { Val::I64(0) } else { write(p, v.as_bytes())? };
            Ok(())
        }),
        Function::new("fc_emit_event", [PTR], [PTR], UserData::new(()), |p, _i, o, _ud| {
            o[0] = write(p, br#"{"ok":true}"#)?;
            Ok(())
        }),
    ];
    if !shadow {
        return fns;
    }
    let http = Arc::new(HttpState::default());
    let (h1, h2, h3) = (http.clone(), http.clone(), http);
    let c1 = ctx.clone();
    fns.push(
        Function::new("http_request", [PTR, PTR], [PTR], UserData::new(()), move |p, i, o, _ud| {
            let raw = read(p, &i[0])?;
            free(p, &i[0])?;
            let body = read(p, &i[1])?;
            free(p, &i[1])?;
            let v: serde_json::Value = serde_json::from_slice(&raw).unwrap_or_default();
            let url = v["url"].as_str().unwrap_or("").to_string();
            let method = v["method"].as_str().unwrap_or("GET").to_uppercase();
            let headers: Vec<(String, String)> = v["headers"]
                .as_object()
                .map(|m| m.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect())
                .unwrap_or_default();
            // The call's deadline: extism exposes the manifest timeout's remainder.
            let deadline = Instant::now() + p.time_remaining().unwrap_or(policy::DEFAULT_CALL_TIMEOUT);
            let (status, hdrs, out) =
                match policy::send_blocking(&c1.policy, &method, &url, &headers, &body, deadline) {
                    Ok(r) => (r.status as i32, r.headers, r.body),
                    Err(why) => (0, Vec::new(), policy::denial_body(&why)),
                };
            h1.last.lock().unwrap().insert(p.id().as_u128(), (status, hdrs));
            o[0] = write(p, &out)?;
            Ok(())
        })
        .with_namespace(ENV),
    );
    fns.push(
        Function::new("http_status_code", [], [ValType::I32], UserData::new(()), move |p, _i, o, _ud| {
            let s = h2.last.lock().unwrap().get(&p.id().as_u128()).map(|x| x.0).unwrap_or(0);
            o[0] = Val::I32(s);
            Ok(())
        })
        .with_namespace(ENV),
    );
    fns.push(
        Function::new("http_headers", [], [PTR], UserData::new(()), move |p, _i, o, _ud| {
            let h = h3.last.lock().unwrap().get(&p.id().as_u128()).map(|x| x.1.clone()).unwrap_or_default();
            o[0] = write(p, &policy::headers_json(&h))?;
            Ok(())
        })
        .with_namespace(ENV),
    );
    for (name, level) in [("log_info", "INFO"), ("log_warn", "WARN"), ("log_error", "ERROR"), ("log_debug", "DEBUG")] {
        let c = ctx.clone();
        fns.push(
            Function::new(name, [PTR], [], UserData::new(()), move |p, i, _o, _ud| {
                let msg = read(p, &i[0])?;
                free(p, &i[0])?;
                c.logs.push(level, &String::from_utf8_lossy(&msg));
                Ok(())
            })
            .with_namespace(ENV),
        );
    }
    fns.push(
        Function::new("get_log_level", [], [ValType::I32], UserData::new(()), |_p, _i, o, _ud| {
            o[0] = Val::I32(2); // INFO
            Ok(())
        })
        .with_namespace(ENV),
    );
    fns
}

pub fn load(wasm: &[u8], ctx: Arc<FnCtx>, cfg: &ExCfg) -> Result<ExFunc> {
    let mut m = Manifest::new([Wasm::data(wasm.to_vec())]).with_timeout(cfg.timeout);
    if let Some(p) = cfg.max_pages {
        m = m.with_memory_max(p);
    }
    for (k, v) in &ctx.config {
        m = m.with_config_key(k, v);
    }
    let mut b = PluginBuilder::new(m).with_wasi(true).with_functions(functions(ctx, cfg.shadow));
    b = match &cfg.cache_dir {
        None => b.with_cache_disabled(),
        Some(dir) => b.with_cache_config(dir),
    };
    if let Some(mb) = cfg.reserve_mb {
        let mut c = extism_wasmtime_config();
        c.memory_reservation(mb << 20).memory_may_move(false).memory_reservation_for_growth(0).memory_guard_size(64 << 10);
        b = b.with_wasmtime_config(c);
    }
    Ok(ExFunc { compiled: CompiledPlugin::new(b)?, pool: Mutex::new(Vec::new()) })
}

/// The wasmtime `Config` type the extism crate links against (wasmtime 43, not 49).
fn extism_wasmtime_config() -> wasmtime43::Config {
    wasmtime43::Config::new()
}

pub fn instantiate(f: &ExFunc) -> Result<Plugin> {
    Plugin::new_from_compiled(&f.compiled)
}

pub fn call(p: &mut Plugin, export: &str, input: &[u8]) -> Result<Vec<u8>> {
    p.call::<&[u8], Vec<u8>>(export, input)
}

pub fn pooled_call(f: &ExFunc, export: &str, input: &[u8]) -> Result<Vec<u8>> {
    let idle = f.pool.lock().unwrap().pop();
    let mut p = match idle {
        Some(p) => p,
        None => instantiate(f)?,
    };
    let r = call(&mut p, export, input);
    if r.is_ok() {
        f.pool.lock().unwrap().push(p);
    }
    r
}
