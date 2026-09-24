//! Variant (b): plain wasmtime running the Extism kernel (`extism-runtime.wasm`, the copy Java's
//! extism-endive ships). One `Engine` for every function; the kernel compiled once per process;
//! one `Module` + `InstancePre` per function version; the pooling allocator; epoch interruption
//! for deadlines; `StoreLimits` capping EACH memory (guest and kernel) at `wasmMemoryMb`.
//!
//! The guest's `extism:host/env` imports are host functions: the kernel's exports are reached
//! through the store's kernel instance (alloc/free/length/input/output/error/reset), the
//! load/store family reads kernel memory directly, and the Extism built-ins (config, var, log,
//! http) plus the FlowCatalyst `extism:host/user` functions are implemented here.

use crate::policy::{self, FnCtx};
use crate::wasi_out::LogOut;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wasmtime::*;
use wasmtime_wasi::p1::WasiP1Ctx;

const ENV: &str = "extism:host/env";
const USER: &str = "extism:host/user";
pub const TICK: Duration = Duration::from_millis(1);

/// The epoch period actually used: `FC_TICK_US`, else `TICK`.
pub fn tick() -> Duration {
    std::env::var("FC_TICK_US").ok().and_then(|v| v.parse().ok()).map(Duration::from_micros).unwrap_or(TICK)
}

pub struct KernelEngine {
    pub engine: Engine,
    linker: Linker<KState>,
    /// None = (b') native kernel.
    kernel_pre: Option<InstancePre<KState>>,
}

#[derive(Clone)]
struct KFns {
    mem: Memory,
    alloc: TypedFunc<u64, u64>,
    free: TypedFunc<u64, ()>,
    length: TypedFunc<u64, u64>,
    length_unsafe: TypedFunc<u64, u64>,
    input_set: TypedFunc<(u64, u64), ()>,
    input_length: TypedFunc<(), u64>,
    input_offset: TypedFunc<(), u64>,
    output_set: TypedFunc<(u64, u64), ()>,
    output_length: TypedFunc<(), u64>,
    output_offset: TypedFunc<(), u64>,
    reset: TypedFunc<(), ()>,
    error_set: TypedFunc<u64, ()>,
    error_get: TypedFunc<(), u64>,
    memory_bytes: TypedFunc<(), u64>,
    load_u8: TypedFunc<u64, u32>,
    load_u64: TypedFunc<u64, u64>,
    input_load_u8: TypedFunc<u64, u32>,
    input_load_u64: TypedFunc<u64, u64>,
    store_u8: TypedFunc<(u64, u32), ()>,
    store_u64: TypedFunc<(u64, u64), ()>,
}

pub struct KState {
    wasi: WasiP1Ctx,
    stdout: LogOut,
    stderr: LogOut,
    limits: StoreLimits,
    k: Option<KFns>,
    nk: Option<NativeKernel>,
    input_offset: u64,
    http_status: i32,
    http_headers: Vec<(String, String)>,
    deadline: Instant,
    ctx: Arc<FnCtx>,
}

/// (b') The Extism kernel's contract implemented in the host (no kernel wasm instance): a
/// bump allocator over a host `Vec<u8>` capped at `wasmMemoryMb`; offsets are 1-based-ish (0 is
/// null); `reset` clears everything; loads/stores are bounds-checked against allocated space
/// (an out-of-bounds load reads 0, an out-of-bounds store is dropped — as the kernel does).
pub struct NativeKernel {
    data: Vec<u8>,
    pos: usize,
    cap: usize,
    blocks: std::collections::HashMap<u64, u64>,
    input: (u64, u64),
    output: (u64, u64),
    error: u64,
}

const NK_BASE: usize = 8;

impl NativeKernel {
    fn new(cap: usize) -> Self {
        Self { data: vec![0; NK_BASE], pos: NK_BASE, cap, blocks: Default::default(), input: (0, 0), output: (0, 0), error: 0 }
    }
    fn reset(&mut self) {
        self.pos = NK_BASE;
        self.blocks.clear();
        self.input = (0, 0);
        self.output = (0, 0);
        self.error = 0;
    }
    fn alloc(&mut self, n: u64) -> u64 {
        let n = n as usize;
        let aligned = (n + 7) & !7;
        if self.pos + aligned > self.cap {
            return 0;
        }
        let off = self.pos;
        self.pos += aligned.max(8);
        if self.data.len() < self.pos {
            self.data.resize(self.pos, 0);
        }
        self.blocks.insert(off as u64, n as u64);
        off as u64
    }
    fn length(&self, off: u64) -> u64 {
        self.blocks.get(&off).copied().unwrap_or(0)
    }
    fn ok(&self, off: u64, w: usize) -> bool {
        off as usize >= NK_BASE && (off as usize).saturating_add(w) <= self.pos
    }
    fn load_u8(&self, off: u64) -> u32 {
        if self.ok(off, 1) { self.data[off as usize] as u32 } else { 0 }
    }
    fn load_u64(&self, off: u64) -> u64 {
        if self.ok(off, 8) { u64::from_le_bytes(self.data[off as usize..off as usize + 8].try_into().unwrap()) } else { 0 }
    }
    fn store_u8(&mut self, off: u64, v: u32) {
        if self.ok(off, 1) {
            self.data[off as usize] = v as u8;
        }
    }
    fn store_u64(&mut self, off: u64, v: u64) {
        if self.ok(off, 8) {
            self.data[off as usize..off as usize + 8].copy_from_slice(&v.to_le_bytes());
        }
    }
    fn bytes(&self, off: u64) -> &[u8] {
        let n = self.length(off) as usize;
        &self.data[off as usize..off as usize + n]
    }
    fn write(&mut self, bytes: &[u8]) -> u64 {
        if bytes.is_empty() {
            return 0;
        }
        let off = self.alloc(bytes.len() as u64);
        if off != 0 {
            self.data[off as usize..off as usize + bytes.len()].copy_from_slice(bytes);
        }
        off
    }
}

pub struct KFunc {
    pub pre: InstancePre<KState>,
    pub ctx: Arc<FnCtx>,
    pub mem_cap: usize,
    pub pool: Mutex<Vec<KInst>>,
}

pub struct KInst {
    store: Store<KState>,
    guest: Instance,
}

/// The engine config shared by (b) and (c): pooling allocator sized for `slots` live stores.
pub fn pooled_config(slots: u32, instances_per_store: u32, mems_per_store: u32, mem_cap: usize) -> Config {
    let mut pool = PoolingAllocationConfig::new();
    pool.total_core_instances(slots * instances_per_store)
        .total_memories(slots * mems_per_store)
        .total_tables(slots * instances_per_store)
        .total_component_instances(slots)
        .total_stacks(slots.min(10_000))
        .max_memory_size(mem_cap)
        .max_core_instances_per_component(32)
        .max_memories_per_component(8)
        .max_tables_per_component(32);
    let mut c = Config::new();
    if std::env::var_os("FC_ONDEMAND").is_none() {
        c.allocation_strategy(InstanceAllocationStrategy::Pooling(pool));
    }
    c.epoch_interruption(true);
    if let Ok(mb) = std::env::var("FC_RESERVE_MB") {
        let bytes: u64 = mb.parse::<u64>().unwrap() << 20;
        c.memory_reservation(bytes).memory_guard_size(64 << 10).memory_reservation_for_growth(0);
    }
    c
}

/// A thread advancing the engine's epoch every `TICK`.
pub fn start_ticker(engine: &Engine) {
    let e = engine.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(tick());
        e.increment_epoch();
    });
}

fn kf(c: &Caller<'_, KState>) -> Result<KFns> {
    c.data().k.clone().ok_or_else(|| format_err!("kernel not instantiated"))
}

fn free_block(c: &mut Caller<'_, KState>, off: u64) -> Result<()> {
    if off == 0 || c.data().nk.is_some() {
        return Ok(());
    }
    kf(c)?.free.call(&mut *c, off)
}

fn read_block(c: &mut Caller<'_, KState>, off: u64) -> Result<Vec<u8>> {
    if off == 0 {
        return Ok(Vec::new());
    }
    if let Some(nk) = &c.data().nk {
        return Ok(nk.bytes(off).to_vec());
    }
    let k = kf(c)?;
    let len = k.length.call(&mut *c, off)? as usize;
    let data = k.mem.data(&*c);
    let (s, e) = (off as usize, off as usize + len);
    if e > data.len() {
        bail!("block out of bounds");
    }
    Ok(data[s..e].to_vec())
}

/// Allocate a kernel block and copy `bytes` in; 0 when empty or when the kernel is out of memory.
fn write_block(c: &mut Caller<'_, KState>, bytes: &[u8]) -> Result<u64> {
    if bytes.is_empty() {
        return Ok(0);
    }
    if let Some(nk) = &mut c.data_mut().nk {
        return Ok(nk.write(bytes));
    }
    let k = kf(c)?;
    let off = k.alloc.call(&mut *c, bytes.len() as u64)?;
    if off == 0 {
        return Ok(0);
    }
    k.mem.write(&mut *c, off as usize, bytes)?;
    Ok(off)
}

fn mem_slice<'a>(data: &'a [u8], off: u64, n: usize) -> Result<&'a [u8]> {
    let s = off as usize;
    data.get(s..s + n).ok_or_else(|| format_err!("kernel memory access out of bounds"))
}

impl KernelEngine {
    pub fn new(kernel_wasm: Option<&[u8]>, slots: u32, mem_cap: usize) -> Result<Self> {
        let engine = Engine::new(&pooled_config(slots, 2, 2, mem_cap))?;
        start_ticker(&engine);
        let mut l: Linker<KState> = Linker::new(&engine);
        wasmtime_wasi::p1::add_to_linker_sync(&mut l, |s: &mut KState| &mut s.wasi)?;
        let kernel_pre = match kernel_wasm {
            Some(kw) => {
                let kernel = Module::new(&engine, kw)?;
                let kernel_linker: Linker<KState> = Linker::new(&engine);
                Some(kernel_linker.instantiate_pre(&kernel)?)
            }
            None => None,
        };
        if kernel_pre.is_none() {
            Self::define_native(&mut l)?;
        } else {

        // Kernel exports, forwarded through the store's kernel instance.
        macro_rules! fwd1 {
            ($name:literal, $field:ident, $a:ty => $r:ty) => {
                l.func_wrap(ENV, $name, |mut c: Caller<'_, KState>, a: $a| -> Result<$r> {
                    let k = kf(&c)?;
                    k.$field.call(&mut c, a)
                })?;
            };
        }
        fwd1!("alloc", alloc, u64 => u64);
        fwd1!("free", free, u64 => ());
        fwd1!("length", length, u64 => u64);
        fwd1!("length_unsafe", length_unsafe, u64 => u64);
        fwd1!("error_set", error_set, u64 => ());
        l.func_wrap(ENV, "input_length", |mut c: Caller<'_, KState>| -> Result<u64> {
            let k = kf(&c)?;
            k.input_length.call(&mut c, ())
        })?;
        l.func_wrap(ENV, "input_offset", |mut c: Caller<'_, KState>| -> Result<u64> {
            let k = kf(&c)?;
            k.input_offset.call(&mut c, ())
        })?;
        l.func_wrap(ENV, "output_set", |mut c: Caller<'_, KState>, o: u64, n: u64| -> Result<()> {
            let k = kf(&c)?;
            k.output_set.call(&mut c, (o, n))
        })?;
        l.func_wrap(ENV, "input_set", |mut c: Caller<'_, KState>, o: u64, n: u64| -> Result<()> {
            let k = kf(&c)?;
            k.input_set.call(&mut c, (o, n))
        })?;
        l.func_wrap(ENV, "error_get", |mut c: Caller<'_, KState>| -> Result<u64> {
            let k = kf(&c)?;
            k.error_get.call(&mut c, ())
        })?;
        l.func_wrap(ENV, "memory_bytes", |mut c: Caller<'_, KState>| -> Result<u64> {
            let k = kf(&c)?;
            k.memory_bytes.call(&mut c, ())
        })?;
        l.func_wrap(ENV, "reset", |mut c: Caller<'_, KState>| -> Result<()> {
            let k = kf(&c)?;
            k.reset.call(&mut c, ())
        })?;

        // Load/store forwarded to the kernel too: its exports bounds-check against the kernel's
        // allocated blocks (a direct memory access here corrupted the kernel on a failed alloc).
        fwd1!("load_u8", load_u8, u64 => u32);
        fwd1!("load_u64", load_u64, u64 => u64);
        fwd1!("input_load_u8", input_load_u8, u64 => u32);
        fwd1!("input_load_u64", input_load_u64, u64 => u64);
        l.func_wrap(ENV, "store_u8", |mut c: Caller<'_, KState>, off: u64, v: u32| -> Result<()> {
            let k = kf(&c)?;
            k.store_u8.call(&mut c, (off, v))
        })?;
        l.func_wrap(ENV, "store_u64", |mut c: Caller<'_, KState>, off: u64, v: u64| -> Result<()> {
            let k = kf(&c)?;
            k.store_u64.call(&mut c, (off, v))
        })?;
        }

        // Extism built-ins.
        l.func_wrap(ENV, "config_get", |mut c: Caller<'_, KState>, key: u64| -> Result<u64> {
            let k = read_block(&mut c, key)?;
            free_block(&mut c, key)?;
            let key = String::from_utf8_lossy(&k).to_string();
            let v = c.data().ctx.config.get(&key).cloned();
            match v {
                Some(v) => write_block(&mut c, v.as_bytes()),
                None => Ok(0),
            }
        })?;
        l.func_wrap(ENV, "var_get", |_c: Caller<'_, KState>, _k: u64| -> u64 { 0 })?;
        l.func_wrap(ENV, "var_set", |_c: Caller<'_, KState>, _k: u64, _v: u64| {})?;
        for (name, level) in [
            ("log_trace", "TRACE"),
            ("log_debug", "DEBUG"),
            ("log_info", "INFO"),
            ("log_warn", "WARN"),
            ("log_error", "ERROR"),
        ] {
            l.func_wrap(ENV, name, move |mut c: Caller<'_, KState>, off: u64| -> Result<()> {
                let msg = read_block(&mut c, off)?;
                free_block(&mut c, off)?;
                c.data().ctx.logs.push(level, &String::from_utf8_lossy(&msg));
                Ok(())
            })?;
        }
        l.func_wrap(ENV, "get_log_level", |_c: Caller<'_, KState>| -> i32 { 2 /* INFO */ })?;
        l.func_wrap(ENV, "http_request", |mut c: Caller<'_, KState>, req: u64, body: u64| -> Result<u64> {
            let raw = read_block(&mut c, req)?;
            free_block(&mut c, req)?;
            let body_bytes = read_block(&mut c, body)?;
            free_block(&mut c, body)?;
            let v: serde_json::Value = serde_json::from_slice(&raw).unwrap_or_default();
            let url = v["url"].as_str().unwrap_or("").to_string();
            let method = v["method"].as_str().unwrap_or("GET").to_uppercase();
            let headers: Vec<(String, String)> = v["headers"]
                .as_object()
                .map(|m| m.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect())
                .unwrap_or_default();
            let (ctx, deadline) = (c.data().ctx.clone(), c.data().deadline);
            let out = match policy::send_blocking(&ctx.policy, &method, &url, &headers, &body_bytes, deadline) {
                Ok(r) => {
                    c.data_mut().http_status = r.status as i32;
                    c.data_mut().http_headers = r.headers;
                    r.body
                }
                Err(why) => {
                    c.data_mut().http_status = 0;
                    c.data_mut().http_headers = Vec::new();
                    policy::denial_body(&why)
                }
            };
            write_block(&mut c, &out)
        })?;
        l.func_wrap(ENV, "http_status_code", |c: Caller<'_, KState>| -> i32 { c.data().http_status })?;
        l.func_wrap(ENV, "http_headers", |mut c: Caller<'_, KState>| -> Result<u64> {
            let h = policy::headers_json(&c.data().http_headers);
            write_block(&mut c, &h)
        })?;

        // FlowCatalyst extism:host/user functions (stubs sufficient for the spike).
        l.func_wrap(USER, "fc_secret_get", |mut c: Caller<'_, KState>, key: u64| -> Result<u64> {
            let k = String::from_utf8_lossy(&read_block(&mut c, key)?).to_string();
            let v = c.data().ctx.secrets.get(&k).cloned().unwrap_or_default();
            write_block(&mut c, v.as_bytes())
        })?;
        l.func_wrap(USER, "fc_emit_event", |mut c: Caller<'_, KState>, _ev: u64| -> Result<u64> {
            write_block(&mut c, br#"{"ok":true}"#)
        })?;

        Ok(Self { engine, linker: l, kernel_pre })
    }

    fn define_native(l: &mut Linker<KState>) -> Result<()> {
        fn nk<'a>(c: &'a mut Caller<'_, KState>) -> &'a mut NativeKernel {
            c.data_mut().nk.as_mut().unwrap()
        }
        l.func_wrap(ENV, "alloc", |mut c: Caller<'_, KState>, n: u64| nk(&mut c).alloc(n))?;
        l.func_wrap(ENV, "free", |_c: Caller<'_, KState>, _o: u64| {})?;
        l.func_wrap(ENV, "length", |mut c: Caller<'_, KState>, o: u64| nk(&mut c).length(o))?;
        l.func_wrap(ENV, "length_unsafe", |mut c: Caller<'_, KState>, o: u64| nk(&mut c).length(o))?;
        l.func_wrap(ENV, "load_u8", |mut c: Caller<'_, KState>, o: u64| nk(&mut c).load_u8(o))?;
        l.func_wrap(ENV, "load_u64", |mut c: Caller<'_, KState>, o: u64| nk(&mut c).load_u64(o))?;
        l.func_wrap(ENV, "input_load_u8", |mut c: Caller<'_, KState>, o: u64| {
            let k = nk(&mut c);
            if o < k.input.1 { k.load_u8(k.input.0 + o) } else { 0 }
        })?;
        l.func_wrap(ENV, "input_load_u64", |mut c: Caller<'_, KState>, o: u64| {
            let k = nk(&mut c);
            if o + 8 <= k.input.1 { k.load_u64(k.input.0 + o) } else { 0 }
        })?;
        l.func_wrap(ENV, "store_u8", |mut c: Caller<'_, KState>, o: u64, v: u32| nk(&mut c).store_u8(o, v))?;
        l.func_wrap(ENV, "store_u64", |mut c: Caller<'_, KState>, o: u64, v: u64| nk(&mut c).store_u64(o, v))?;
        l.func_wrap(ENV, "input_set", |mut c: Caller<'_, KState>, o: u64, n: u64| nk(&mut c).input = (o, n))?;
        l.func_wrap(ENV, "input_length", |mut c: Caller<'_, KState>| nk(&mut c).input.1)?;
        l.func_wrap(ENV, "input_offset", |mut c: Caller<'_, KState>| nk(&mut c).input.0)?;
        l.func_wrap(ENV, "output_set", |mut c: Caller<'_, KState>, o: u64, n: u64| nk(&mut c).output = (o, n))?;
        l.func_wrap(ENV, "error_set", |mut c: Caller<'_, KState>, o: u64| nk(&mut c).error = o)?;
        l.func_wrap(ENV, "error_get", |mut c: Caller<'_, KState>| nk(&mut c).error)?;
        l.func_wrap(ENV, "memory_bytes", |mut c: Caller<'_, KState>| nk(&mut c).pos as u64)?;
        l.func_wrap(ENV, "reset", |mut c: Caller<'_, KState>| nk(&mut c).reset())?;
        Ok(())
    }

    pub fn compile(&self, wasm: &[u8]) -> Result<Module> {
        Module::new(&self.engine, wasm)
    }

    pub fn precompile(&self, wasm: &[u8]) -> Result<Vec<u8>> {
        self.engine.precompile_module(wasm)
    }

    pub fn deserialize_file(&self, path: &Path) -> Result<Module> {
        unsafe { Module::deserialize_file(&self.engine, path) }
    }

    pub fn load(&self, module: &Module, ctx: Arc<FnCtx>, mem_cap: usize) -> Result<KFunc> {
        let pre = self.linker.instantiate_pre(module)?;
        Ok(KFunc { pre, ctx, mem_cap, pool: Mutex::new(Vec::new()) })
    }

    pub fn instantiate(&self, f: &KFunc) -> Result<KInst> {
        let stdout = LogOut::new("INFO", f.ctx.logs.clone());
        let stderr = LogOut::new("WARN", f.ctx.logs.clone());
        let wasi = wasmtime_wasi::WasiCtxBuilder::new().stdout(stdout.clone()).stderr(stderr.clone()).build_p1();
        let state = KState {
            wasi,
            stdout,
            stderr,
            limits: StoreLimitsBuilder::new().memory_size(f.mem_cap).instances(4).build(),
            k: None,
            nk: if self.kernel_pre.is_none() { Some(NativeKernel::new(f.mem_cap)) } else { None },
            input_offset: 0,
            http_status: 0,
            http_headers: Vec::new(),
            deadline: Instant::now(),
            ctx: f.ctx.clone(),
        };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|s| &mut s.limits);
        store.set_epoch_deadline(u64::MAX / 2);
        store.epoch_deadline_trap();
        if let Some(kernel_pre) = &self.kernel_pre {
        let kernel = kernel_pre.instantiate(&mut store)?;
        macro_rules! t {
            ($n:literal) => {
                kernel.get_typed_func(&mut store, $n)?
            };
        }
        let k = KFns {
            mem: kernel.get_memory(&mut store, "memory").ok_or_else(|| format_err!("kernel memory"))?,
            alloc: t!("alloc"),
            free: t!("free"),
            length: t!("length"),
            length_unsafe: t!("length_unsafe"),
            input_set: t!("input_set"),
            input_length: t!("input_length"),
            input_offset: t!("input_offset"),
            output_set: t!("output_set"),
            output_length: t!("output_length"),
            output_offset: t!("output_offset"),
            reset: t!("reset"),
            error_set: t!("error_set"),
            error_get: t!("error_get"),
            memory_bytes: t!("memory_bytes"),
            load_u8: t!("load_u8"),
            load_u64: t!("load_u64"),
            input_load_u8: t!("input_load_u8"),
            input_load_u64: t!("input_load_u64"),
            store_u8: t!("store_u8"),
            store_u64: t!("store_u64"),
        };
        store.data_mut().k = Some(k);
        }
        let guest = f.pre.instantiate(&mut store)?;
        if let Some(init) = guest.get_typed_func::<(), ()>(&mut store, "_initialize").ok() {
            init.call(&mut store, ())?;
        }
        Ok(KInst { store, guest })
    }

    /// One call on an instance. Any `Err` means the instance must be discarded.
    pub fn call(&self, inst: &mut KInst, export: &str, input: &[u8], timeout: Duration) -> Result<Vec<u8>> {
        let s = &mut inst.store;
        if s.data().nk.is_some() {
            return Self::call_native(inst, export, input, timeout);
        }
        let k = s.data().k.clone().unwrap();
        k.reset.call(&mut *s, ())?;
        let off = k.alloc.call(&mut *s, input.len() as u64)?;
        if off == 0 && !input.is_empty() {
            bail!("kernel out of memory for the input");
        }
        k.mem.write(&mut *s, off as usize, input)?;
        k.input_set.call(&mut *s, (off, input.len() as u64))?;
        s.data_mut().input_offset = off;
        s.data_mut().deadline = Instant::now() + timeout;
        let ticks = (timeout.as_micros() / tick().as_micros()).max(1) as u64;
        s.set_epoch_deadline(ticks);
        let f = inst.guest.get_typed_func::<(), i32>(&mut *s, export)?;
        let rc = f.call(&mut *s, ());
        s.set_epoch_deadline(u64::MAX / 2);
        s.data().stdout.flush_line();
        s.data().stderr.flush_line();
        let rc = rc?;
        if rc != 0 {
            let e = k.error_get.call(&mut *s, ())?;
            let msg = if e == 0 {
                String::new()
            } else {
                let n = k.length.call(&mut *s, e)? as usize;
                String::from_utf8_lossy(&k.mem.data(&*s)[e as usize..e as usize + n]).to_string()
            };
            bail!("guest returned {rc}: {msg}");
        }
        let (o, n) = (k.output_offset.call(&mut *s, ())?, k.output_length.call(&mut *s, ())?);
        Ok(k.mem.data(&*s)[o as usize..(o + n) as usize].to_vec())
    }

    fn call_native(inst: &mut KInst, export: &str, input: &[u8], timeout: Duration) -> Result<Vec<u8>> {
        let s = &mut inst.store;
        {
            let nk = s.data_mut().nk.as_mut().unwrap();
            nk.reset();
            let off = nk.write(input);
            if off == 0 && !input.is_empty() {
                bail!("kernel out of memory for the input");
            }
            nk.input = (off, input.len() as u64);
        }
        s.data_mut().deadline = Instant::now() + timeout;
        let ticks = (timeout.as_micros() / tick().as_micros()).max(1) as u64;
        s.set_epoch_deadline(ticks);
        let f = inst.guest.get_typed_func::<(), i32>(&mut *s, export)?;
        let rc = f.call(&mut *s, ());
        s.set_epoch_deadline(u64::MAX / 2);
        s.data().stdout.flush_line();
        s.data().stderr.flush_line();
        let rc = rc?;
        let nk = s.data().nk.as_ref().unwrap();
        if rc != 0 {
            let msg = if nk.error == 0 { String::new() } else { String::from_utf8_lossy(nk.bytes(nk.error)).to_string() };
            bail!("guest returned {rc}: {msg}");
        }
        let (o, n) = nk.output;
        Ok(nk.data[o as usize..(o + n) as usize].to_vec())
    }

    /// Borrow an idle instance (or make one), call, give it back on success, drop it on failure.
    pub fn pooled_call(&self, f: &KFunc, export: &str, input: &[u8], timeout: Duration) -> Result<Vec<u8>> {
        let idle = f.pool.lock().unwrap().pop();
        let mut inst = match idle {
            Some(i) => i,
            None => self.instantiate(f)?,
        };
        let r = self.call(&mut inst, export, input, timeout);
        if r.is_ok() {
            f.pool.lock().unwrap().push(inst);
        }
        r
    }

    /// Instance-per-request: a fresh instance for every call, dropped after.
    pub fn fresh_call(&self, f: &KFunc, export: &str, input: &[u8], timeout: Duration) -> Result<Vec<u8>> {
        let mut inst = self.instantiate(f)?;
        self.call(&mut inst, export, input, timeout)
    }
}
