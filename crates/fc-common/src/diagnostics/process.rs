//! Process figures: CPU, resident memory, file descriptors, threads.
//!
//! Read straight from the kernel with no dependency beyond `libc`: on
//! Linux (production) `/proc/self/*`, elsewhere what POSIX offers
//! (`getrusage`, `getrlimit`, `/dev/fd`). A figure the platform cannot
//! give is `None` and is left out of the exposition rather than reported
//! as zero.

use serde::Serialize;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// A point-in-time reading of the process.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessSnapshot {
    /// User + system CPU time since the process started.
    pub cpu_seconds: Option<f64>,
    /// Resident set size now (Linux only).
    pub resident_memory_bytes: Option<u64>,
    /// Peak resident set size.
    pub max_resident_memory_bytes: Option<u64>,
    /// Virtual memory size (Linux only).
    pub virtual_memory_bytes: Option<u64>,
    pub open_fds: Option<u64>,
    /// The soft `RLIMIT_NOFILE`.
    pub max_fds: Option<u64>,
    /// OS threads (Linux only).
    pub threads: Option<u64>,
    /// Unix time the process started (see [`mark_start`]).
    pub start_time_seconds: f64,
}

static START: OnceLock<f64> = OnceLock::new();

/// Record the process start time. Called by [`super::init`]; later calls
/// keep the first value. Without it, the first snapshot's time is used.
pub fn mark_start() {
    START.get_or_init(now_secs);
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Read the process figures now. Cheap: a few small `/proc` reads on
/// Linux, two syscalls elsewhere.
pub fn snapshot() -> ProcessSnapshot {
    let mut s = ProcessSnapshot {
        start_time_seconds: *START.get_or_init(now_secs),
        ..ProcessSnapshot::default()
    };
    posix(&mut s);
    linux(&mut s);
    s
}

#[cfg(unix)]
fn posix(s: &mut ProcessSnapshot) {
    // SAFETY: getrusage/getrlimit only write into the zeroed structs we
    // hand them.
    unsafe {
        let mut usage: libc::rusage = std::mem::zeroed();
        if libc::getrusage(libc::RUSAGE_SELF, &mut usage) == 0 {
            let secs = |tv: libc::timeval| tv.tv_sec as f64 + tv.tv_usec as f64 / 1_000_000.0;
            s.cpu_seconds = Some(secs(usage.ru_utime) + secs(usage.ru_stime));
            // Linux reports kilobytes, the BSDs (macOS) bytes.
            let max_rss = usage.ru_maxrss.max(0) as u64;
            s.max_resident_memory_bytes = Some(if cfg!(target_os = "linux") {
                max_rss * 1024
            } else {
                max_rss
            });
        }
        let mut limit: libc::rlimit = std::mem::zeroed();
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) == 0
            && limit.rlim_cur != libc::RLIM_INFINITY
        {
            s.max_fds = Some(limit.rlim_cur as u64);
        }
    }
    if !cfg!(target_os = "linux") {
        // macOS and the BSDs list the process's descriptors under /dev/fd
        // (the listing's own descriptor included, hence the -1).
        s.open_fds = count_dir("/dev/fd").map(|n| n.saturating_sub(1));
    }
}

#[cfg(not(unix))]
fn posix(_: &mut ProcessSnapshot) {}

#[cfg(target_os = "linux")]
fn linux(s: &mut ProcessSnapshot) {
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        let status = parse_status(&status);
        s.resident_memory_bytes = status.rss_bytes;
        s.virtual_memory_bytes = status.vm_bytes;
        s.threads = status.threads;
    }
    s.open_fds = count_dir("/proc/self/fd");
}

#[cfg(not(target_os = "linux"))]
fn linux(_: &mut ProcessSnapshot) {}

#[cfg(unix)]
fn count_dir(path: &str) -> Option<u64> {
    std::fs::read_dir(path).ok().map(|d| d.count() as u64)
}

/// What `/proc/self/status` says about memory and threads.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Status {
    pub rss_bytes: Option<u64>,
    pub vm_bytes: Option<u64>,
    pub threads: Option<u64>,
}

/// Parse `/proc/<pid>/status` (`VmRSS:  1234 kB`, `Threads:  9`).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn parse_status(raw: &str) -> Status {
    let mut out = Status::default();
    for line in raw.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let number = || {
            value
                .split_whitespace()
                .next()
                .and_then(|n| n.parse::<u64>().ok())
        };
        match key {
            "VmRSS" => out.rss_bytes = number().map(|kb| kb * 1024),
            "VmSize" => out.vm_bytes = number().map(|kb| kb * 1024),
            "Threads" => out.threads = number(),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proc_status() {
        let raw = "Name:\tfc-server\nVmSize:\t  204800 kB\nVmRSS:\t   51200 kB\nThreads:\t12\n";
        assert_eq!(
            parse_status(raw),
            Status {
                rss_bytes: Some(51200 * 1024),
                vm_bytes: Some(204800 * 1024),
                threads: Some(12),
            }
        );
    }

    #[test]
    fn a_snapshot_reads_what_the_platform_offers() {
        let s = snapshot();
        assert!(s.start_time_seconds > 0.0);
        #[cfg(unix)]
        {
            assert!(s.cpu_seconds.is_some());
            assert!(s.open_fds.unwrap_or(0) > 0);
        }
        #[cfg(target_os = "linux")]
        {
            assert!(s.resident_memory_bytes.unwrap_or(0) > 0);
            assert!(s.threads.unwrap_or(0) > 0);
        }
    }
}
