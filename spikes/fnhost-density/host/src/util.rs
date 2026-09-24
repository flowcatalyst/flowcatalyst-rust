//! Measurement helpers: process memory, percentiles, the ABI request JSON, fixture checks.

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::Duration;

/// (resident bytes, physical footprint bytes) of this process.
///
/// `resident` is macOS `ri_resident_size` (RSS, includes clean file-backed and shared pages).
/// `footprint` is `ri_phys_footprint` (dirty + compressed anonymous memory; what Activity
/// Monitor calls "Memory") — the closer analogue of a Linux container's anon RSS.
pub fn mem() -> (u64, u64) {
    unsafe {
        let mut info: libc::rusage_info_v2 = std::mem::zeroed();
        let rc = libc::proc_pid_rusage(
            libc::getpid(),
            libc::RUSAGE_INFO_V2,
            &mut info as *mut _ as *mut libc::rusage_info_t,
        );
        if rc != 0 {
            return (0, 0);
        }
        (info.ri_resident_size, info.ri_phys_footprint)
    }
}

pub fn mib(b: u64) -> f64 {
    b as f64 / (1024.0 * 1024.0)
}

/// Settle the allocator before sampling memory.
pub fn settle() {
    std::thread::sleep(Duration::from_millis(300));
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Pct {
    pub n: usize,
    pub p50: f64,
    pub p90: f64,
    pub p99: f64,
    pub max: f64,
    pub mean: f64,
}

/// Percentiles in microseconds.
pub fn pct(samples: &mut [Duration]) -> Pct {
    if samples.is_empty() {
        return Pct::default();
    }
    samples.sort_unstable();
    let us = |d: Duration| d.as_secs_f64() * 1e6;
    let at = |q: f64| {
        let i = ((samples.len() as f64 - 1.0) * q).round() as usize;
        us(samples[i])
    };
    let mean = samples.iter().map(|d| us(*d)).sum::<f64>() / samples.len() as f64;
    Pct { n: samples.len(), p50: at(0.50), p90: at(0.90), p99: at(0.99), max: us(*samples.last().unwrap()), mean }
}

impl std::fmt::Display for Pct {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "n={} p50={:.1}us p90={:.1}us p99={:.1}us max={:.1}us mean={:.1}us",
            self.n, self.p50, self.p90, self.p99, self.max, self.mean
        )
    }
}

/// Verify a fixture against a `SHA256SUMS` file sitting beside it.
pub fn verify_sha256(file: &Path) -> Result<Vec<u8>> {
    let bytes = std::fs::read(file).with_context(|| format!("read {}", file.display()))?;
    let sums = file.with_file_name("SHA256SUMS");
    let name = file.file_name().unwrap().to_string_lossy().to_string();
    let text = std::fs::read_to_string(&sums).with_context(|| format!("read {}", sums.display()))?;
    let want = text
        .lines()
        .find_map(|l| {
            let mut it = l.split_whitespace();
            let (h, n) = (it.next()?, it.next()?);
            (n.trim_start_matches('*') == name).then(|| h.to_string())
        })
        .with_context(|| format!("{name} not listed in {}", sums.display()))?;
    let got = hex::encode(Sha256::digest(&bytes));
    if got != want {
        bail!("{name}: sha256 {got} != pinned {want}");
    }
    Ok(bytes)
}

/// The Java host's guest input (docs/spec/function-wasm-runtime.md §3), key order as Jackson
/// writes it. `query` is a raw `a=b&c=d` string.
pub fn abi_request(address: &str, query: &str) -> Vec<u8> {
    let mut q = serde_json::Map::new();
    for pair in query.split('&').filter(|s| !s.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        q.entry(k.to_string())
            .or_insert_with(|| serde_json::Value::Array(vec![]))
            .as_array_mut()
            .unwrap()
            .push(serde_json::Value::String(v.to_string()));
    }
    serde_json::to_vec(&serde_json::json!({
        "address": address,
        "version": 1,
        "invocationId": "inv_0HZXEQ5Y8JY5Z",
        "method": "POST",
        "path": "/x",
        "originalHost": "fn.example.test",
        "originalPath": "/functions/bench.echo/x",
        "pathParams": {},
        "query": q,
        "headers": {"content-type": ["application/json"], "x-request-id": ["r-1"]},
        "bodyBase64": "eyJoZWxsbyI6IndvcmxkIiwibiI6NDJ9",
        "remoteAddress": "127.0.0.1",
        "caller": {"kind": "principal", "id": "prn_0HZXEQ5Y8JY5Z", "type": "SERVICE", "tier": "CLIENT",
                   "clients": ["clt_0HZXEQ5Y8JY5Z"], "roles": ["function-publisher"], "applications": [],
                   "allApplications": false, "permissions": ["platform:function:version:invoke"]}
    }))
    .unwrap()
}

/// A guest's WASI stdout/stderr as log lines (Java `GuestOutputStream`): a line ends at `\n`
/// or at 8 KiB; stdout logs at INFO, stderr at WARN. The spike collects the lines so the
/// egress tests can assert on them.
#[derive(Clone, Default)]
pub struct LineSink {
    pub lines: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl LineSink {
    pub fn push(&self, level: &str, text: &str) {
        if std::env::var_os("FC_DEBUG").is_some() {
            eprintln!("[guest {level}] {text}");
        }
        self.lines.lock().unwrap().push(format!("{level} {text}"));
    }
    pub fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.lines.lock().unwrap())
    }
}

/// Splits a byte stream into lines at `\n` or 8 KiB.
pub struct LineSplitter {
    buf: Vec<u8>,
    level: &'static str,
    sink: LineSink,
}

pub const MAX_LINE_BYTES: usize = 8 * 1024;

impl LineSplitter {
    pub fn new(level: &'static str, sink: LineSink) -> Self {
        Self { buf: Vec::new(), level, sink }
    }
    pub fn write(&mut self, data: &[u8]) {
        for &b in data {
            if b == b'\n' {
                self.emit();
                continue;
            }
            self.buf.push(b);
            if self.buf.len() >= MAX_LINE_BYTES {
                self.emit();
            }
        }
    }
    pub fn flush(&mut self) {
        if !self.buf.is_empty() {
            self.emit();
        }
    }
    fn emit(&mut self) {
        let mut s = String::from_utf8_lossy(&self.buf).to_string();
        if s.ends_with('\r') {
            s.pop();
        }
        self.buf.clear();
        self.sink.push(self.level, &s);
    }
}
