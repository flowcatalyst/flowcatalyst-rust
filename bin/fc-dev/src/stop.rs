//! `fc-dev stop` — Go's `fcdev stop` (`cmd/fcdev/stop.go`).
//!
//! Reads the shared PID file, sends SIGTERM so the running fcdev drains and
//! stops its embedded Postgres, then waits — escalating to SIGKILL only past
//! `--timeout`. The PID file is the one Go and Java use, so this stops
//! whichever of the three is running, and their `fcdev stop` stops fc-dev.
//! Nothing running is not an error; a stale PID file is removed.

use anyhow::{bail, Result};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::instance_guard::{process_alive, read_pid_file, remove_pid_file_if_owned};

#[derive(clap::Args, Debug)]
pub struct StopArgs {
    /// PID file written by the running fcdev.
    /// [default: <userDataDir>/flowcatalyst/fcdev.pid]
    #[arg(long, env = "FC_DEV_PID_FILE", value_name = "FILE")]
    pub pid_file: Option<PathBuf>,

    /// How long to wait for a graceful exit before SIGKILL (e.g. 20s, 1m).
    #[arg(long, default_value = "20s", value_parser = parse_duration)]
    pub timeout: Duration,
}

/// Go's `time.ParseDuration` for the forms used here: `20s`, `1500ms`,
/// `2m`, or bare seconds.
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    let (num, unit) = match s.find(|c: char| !c.is_ascii_digit() && c != '.') {
        Some(i) => (&s[..i], &s[i..]),
        None => (s, "s"),
    };
    let n: f64 = num.parse().map_err(|_| format!("invalid duration {s:?}"))?;
    let secs = match unit {
        "ms" => n / 1000.0,
        "s" => n,
        "m" => n * 60.0,
        "h" => n * 3600.0,
        _ => {
            return Err(format!(
                "invalid duration unit in {s:?} (use ms, s, m or h)"
            ))
        }
    };
    Ok(Duration::from_secs_f64(secs))
}

pub fn run(args: StopArgs) -> Result<()> {
    let pid_file = args
        .pid_file
        .unwrap_or_else(crate::dev_paths::default_pid_file);

    let Some(pid) = read_pid_file(&pid_file)? else {
        println!(
            "No running fcdev instance found (no pid file at {}).",
            pid_file.display()
        );
        return Ok(());
    };
    if !process_alive(pid) {
        let _ = std::fs::remove_file(&pid_file);
        println!("No running fcdev instance (pid {pid} not alive); removed stale pid file.");
        return Ok(());
    }

    println!("Stopping fcdev (pid {pid})…");
    signal(pid, Signal::Term)?;
    if wait_for_exit(pid, args.timeout) {
        remove_pid_file_if_owned(&pid_file, pid);
        println!("Stopped fcdev (pid {pid}).");
        return Ok(());
    }

    println!(
        "fcdev did not exit within {:?}; sending SIGKILL.",
        args.timeout
    );
    signal(pid, Signal::Kill)?;
    if !wait_for_exit(pid, Duration::from_secs(5)) {
        bail!("pid {pid} still running after SIGKILL");
    }
    remove_pid_file_if_owned(&pid_file, pid);
    println!("Force-stopped fcdev (pid {pid}).");
    Ok(())
}

enum Signal {
    Term,
    Kill,
}

#[cfg(unix)]
fn signal(pid: u32, sig: Signal) -> Result<()> {
    let signo = match sig {
        Signal::Term => libc::SIGTERM,
        Signal::Kill => libc::SIGKILL,
    };
    // SAFETY: kill(2) on a PID read from the PID file.
    if unsafe { libc::kill(pid as libc::pid_t, signo) } != 0 {
        bail!("signal pid {pid}: {}", std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(unix))]
fn signal(pid: u32, _sig: Signal) -> Result<()> {
    bail!("`fc-dev stop` is not supported on this platform; stop pid {pid} yourself")
}

/// Poll every 150 ms (Go) until `pid` is gone or `timeout` elapses.
fn wait_for_exit(pid: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !process_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    !process_alive(pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_parse_like_go() {
        assert_eq!(parse_duration("20s").unwrap(), Duration::from_secs(20));
        assert_eq!(
            parse_duration("1500ms").unwrap(),
            Duration::from_millis(1500)
        );
        assert_eq!(parse_duration("2m").unwrap(), Duration::from_secs(120));
        assert_eq!(parse_duration("7").unwrap(), Duration::from_secs(7));
        assert!(parse_duration("soon").is_err());
        assert!(parse_duration("5d").is_err());
    }

    #[test]
    fn nothing_running_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("fcdev.pid");
        run(StopArgs {
            pid_file: Some(pid_file.clone()),
            timeout: Duration::from_secs(1),
        })
        .unwrap();
        std::fs::write(&pid_file, "0\n").unwrap();
        run(StopArgs {
            pid_file: Some(pid_file.clone()),
            timeout: Duration::from_secs(1),
        })
        .unwrap();
        assert!(!pid_file.exists(), "stale pid file removed");
    }

    #[cfg(unix)]
    #[test]
    fn stop_terminates_the_recorded_process() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("fcdev.pid");
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        crate::instance_guard::write_pid_file(&pid_file, child.id()).unwrap();
        // Reap the child as soon as it exits so it isn't a live zombie.
        let pid = child.id();
        let reaper = std::thread::spawn(move || child.wait());
        run(StopArgs {
            pid_file: Some(pid_file.clone()),
            timeout: Duration::from_secs(5),
        })
        .unwrap();
        let _ = reaper.join();
        assert!(!process_alive(pid));
        assert!(!pid_file.exists());
    }
}
