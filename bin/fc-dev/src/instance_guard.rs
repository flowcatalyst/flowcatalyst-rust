//! One fcdev at a time — Rust, Go or Java — on the shared state.
//!
//! Three signals, each shared with the other binaries:
//!
//! - **The PID file** (`<userDataDir>/flowcatalyst/fcdev.pid`, Go's
//!   `pidfile.go`, same format: the decimal PID and a newline, mode 0600).
//!   Go and Java write it unconditionally; fc-dev refuses to start while it
//!   names a live fcdev process, then writes its own, and removes it on exit
//!   only while it still holds fc-dev's PID. `fcdev stop` (any of the three)
//!   finds fc-dev through it.
//! - **`postmaster.pid`** in the cluster: PostgreSQL's own lock. A live PID
//!   there means some binary is serving the cluster right now.
//! - **`epg-lock`** in the cluster: the file zonky (Java's embedded
//!   Postgres) holds a POSIX record lock on for as long as Java's fcdev runs.
//!   fc-dev takes the same lock while it serves the cluster, so a Java fcdev
//!   started meanwhile refuses at once ("could not lock …/epg-lock"), and a
//!   Java fcdev already running is named by its PID.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

/// Go `readPIDFile`: the PID recorded at `path`, `None` when there is none.
pub fn read_pid_file(path: &Path) -> Result<Option<u32>> {
    match std::fs::read_to_string(path) {
        Ok(s) => s
            .trim()
            .parse::<u32>()
            .map(Some)
            .with_context(|| format!("malformed pid file {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("read pid file {}", path.display())),
    }
}

/// Go `writePIDFile`: `<pid>\n`, parent directories created, mode 0600.
pub fn write_pid_file(path: &Path, pid: u32) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("create pid dir")?;
    }
    crate::dev_paths::write_private(path, format!("{pid}\n").as_bytes())
        .with_context(|| format!("write pid file {}", path.display()))
}

/// Go `removePIDFileIfOwned`.
pub fn remove_pid_file_if_owned(path: &Path, pid: u32) {
    if let Ok(Some(cur)) = read_pid_file(path) {
        if cur == pid {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Go `processAlive`: signal 0 checks existence; EPERM means it exists but
/// belongs to another user.
#[cfg(unix)]
pub fn process_alive(pid: u32) -> bool {
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    // SAFETY: kill(2) with signal 0 performs only the existence and
    // permission check; it delivers nothing.
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
pub fn process_alive(_pid: u32) -> bool {
    // No cheap existence check without extra Windows bindings; Go's
    // `stop` has the same limitation there. Treat the file as stale.
    false
}

/// The command a PID runs, for messages (`ps -o command=`), trimmed to a
/// readable length. `None` when it can't be read.
pub fn process_command(pid: u32) -> Option<String> {
    #[cfg(unix)]
    {
        let out = std::process::Command::new("ps")
            .args(["-o", "command=", "-p", &pid.to_string()])
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if s.is_empty() {
            return None;
        }
        Some(if s.chars().count() > 120 {
            format!("{}…", s.chars().take(120).collect::<String>())
        } else {
            s
        })
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        None
    }
}

/// Which fcdev a command line looks like, for messages.
pub fn describe(command: &str) -> &'static str {
    let lower = command.to_ascii_lowercase();
    if lower.contains("fc-dev") {
        "Rust fc-dev"
    } else if lower.contains("java") || lower.contains("io.flowcatalyst") {
        "Java fcdev"
    } else if lower.contains("fcdev") {
        "Go fcdev"
    } else if lower.contains("postgres") {
        "PostgreSQL"
    } else {
        "a process"
    }
}

/// Whether a live PID-file holder is plausibly an fcdev (not a PID reused
/// after a reboot by something unrelated).
fn looks_like_fcdev(command: Option<&str>) -> bool {
    match command {
        // Unreadable: assume it is, so we never run beside a real instance.
        None => true,
        Some(c) => {
            let l = c.to_ascii_lowercase();
            l.contains("fcdev") || l.contains("fc-dev") || l.contains("java")
        }
    }
}

/// The running fc-dev's claim on the PID file, released on drop.
pub struct PidFileGuard {
    path: PathBuf,
    pid: u32,
}

impl Drop for PidFileGuard {
    fn drop(&mut self) {
        remove_pid_file_if_owned(&self.path, self.pid);
    }
}

/// Refuse while the PID file names another live fcdev; otherwise record
/// this process in it. Failing to write the file is a warning, as in Go
/// (DEV-2): `stop` then can't find this instance, nothing else breaks.
pub fn claim_pid_file(path: &Path) -> Result<Option<PidFileGuard>> {
    let me = std::process::id();
    if let Some(pid) = read_pid_file(path).unwrap_or(None) {
        if pid != me && process_alive(pid) {
            let command = process_command(pid);
            if looks_like_fcdev(command.as_deref()) {
                let what = command.as_deref().map(describe).unwrap_or("an fcdev");
                bail!(
                    "{what} is already running (pid {pid}{}), recorded in {}. Rust, Go and Java \
                     fcdev share one embedded cluster and must not run at the same time: stop it \
                     first (`fc-dev stop`, or `fcdev stop` with the Go/Java binary), or pass \
                     --pid-file <other file> if it deliberately uses a different database and ports.",
                    command
                        .map(|c| format!(": {c}"))
                        .unwrap_or_default(),
                    path.display()
                );
            }
            tracing::warn!(
                pid,
                path = %path.display(),
                "PID file names a live process that is not an fcdev (PID reused?); replacing it"
            );
        }
    }
    if let Err(e) = write_pid_file(path, me) {
        tracing::warn!(
            path = %path.display(),
            error = %e,
            "could not write pid file — `fc-dev stop` won't find this instance"
        );
        return Ok(None);
    }
    Ok(Some(PidFileGuard {
        path: path.to_path_buf(),
        pid: me,
    }))
}

/// A live postmaster serving a cluster, read from its `postmaster.pid`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningPostmaster {
    pub pid: u32,
    /// Line 4 of `postmaster.pid`; `None` if the file is still being written.
    pub port: Option<u16>,
}

/// `postmaster.pid` lines: 1 PID, 2 data dir, 3 start time, 4 port, …
pub fn parse_postmaster_pid(contents: &str) -> Option<RunningPostmaster> {
    let mut lines = contents.lines();
    let pid = lines.next()?.trim().parse::<u32>().ok()?;
    let port = lines.nth(2).and_then(|l| l.trim().parse::<u16>().ok());
    Some(RunningPostmaster { pid, port })
}

/// The postmaster currently serving `cluster_dir`, if any (a stale file
/// whose PID is gone is ignored — PostgreSQL replaces it on start).
pub fn running_postmaster(cluster_dir: &Path) -> Option<RunningPostmaster> {
    let contents = std::fs::read_to_string(cluster_dir.join("postmaster.pid")).ok()?;
    let pm = parse_postmaster_pid(&contents)?;
    process_alive(pm.pid).then_some(pm)
}

/// Who serves the cluster, for the refusal message: the PID-file holder
/// (the fcdev that started the server) when it is alive.
pub fn describe_cluster_owner(pid_file: &Path, pm: &RunningPostmaster) -> String {
    let mut s = format!(
        "PostgreSQL pid {}{}",
        pm.pid,
        pm.port.map(|p| format!(" on port {p}")).unwrap_or_default()
    );
    if let Ok(Some(pid)) = read_pid_file(pid_file) {
        if pid != std::process::id() && process_alive(pid) {
            let cmd = process_command(pid);
            let what = cmd.as_deref().map(describe).unwrap_or("an fcdev");
            s.push_str(&format!(", started by {what} (pid {pid})"));
        }
    }
    s
}

/// An exclusive POSIX record lock on `<cluster>/epg-lock`, zonky's lock
/// file. Held for as long as this value lives; the kernel releases it if
/// the process dies.
pub struct ClusterLock {
    #[allow(dead_code)]
    file: std::fs::File,
}

/// Take the `epg-lock` lock, or say who holds it.
#[cfg(unix)]
pub fn lock_cluster(cluster_dir: &Path) -> Result<ClusterLock> {
    use std::os::unix::io::AsRawFd;

    let path = cluster_dir.join("epg-lock");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    // Java's `FileChannel.tryLock()`: an exclusive lock over the whole file.
    // SAFETY: a zeroed flock is a valid "whole file from offset 0" request.
    let mut fl: libc::flock = unsafe { std::mem::zeroed() };
    fl.l_type = libc::F_WRLCK as _;
    fl.l_whence = libc::SEEK_SET as _;
    // SAFETY: fcntl on an fd we own with a valid flock pointer.
    let rc = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETLK, &fl) };
    if rc == 0 {
        return Ok(ClusterLock { file });
    }
    let err = std::io::Error::last_os_error();
    // F_GETLK reports the holder's PID.
    let mut probe: libc::flock = unsafe { std::mem::zeroed() };
    probe.l_type = libc::F_WRLCK as _;
    probe.l_whence = libc::SEEK_SET as _;
    // SAFETY: as above.
    let holder = if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETLK, &mut probe) } == 0
        && i64::from(probe.l_type) != i64::from(libc::F_UNLCK)
    {
        Some(probe.l_pid as u32)
    } else {
        None
    };
    match holder {
        Some(pid) => {
            let cmd = process_command(pid);
            bail!(
                "the embedded cluster is locked by {} (pid {pid}{}) via {}; stop it before starting fc-dev",
                cmd.as_deref().map(describe).unwrap_or("another fcdev"),
                cmd.map(|c| format!(": {c}")).unwrap_or_default(),
                path.display()
            )
        }
        None => bail!("could not lock {}: {err}", path.display()),
    }
}

#[cfg(not(unix))]
pub fn lock_cluster(cluster_dir: &Path) -> Result<ClusterLock> {
    let path = cluster_dir.join("epg-lock");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    Ok(ClusterLock { file })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pid_file_round_trips_in_gos_format() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("fcdev.pid");
        write_pid_file(&path, 4242).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "4242\n");
        assert_eq!(read_pid_file(&path).unwrap(), Some(4242));
        remove_pid_file_if_owned(&path, 1);
        assert!(path.exists(), "someone else's pid file is left alone");
        remove_pid_file_if_owned(&path, 4242);
        assert!(!path.exists());
        assert_eq!(read_pid_file(&path).unwrap(), None);
    }

    #[test]
    fn a_stale_pid_file_is_replaced_and_released_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fcdev.pid");
        // PID 0 is never a live process.
        std::fs::write(&path, "0\n").unwrap();
        let guard = claim_pid_file(&path).unwrap().expect("written");
        assert_eq!(read_pid_file(&path).unwrap(), Some(std::process::id()));
        drop(guard);
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_live_fcdev_in_the_pid_file_refuses_the_start() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fcdev.pid");
        // This test binary is alive; its command line holds "fc_dev".
        // Spawn a process whose command contains "fcdev" instead.
        let mut child = std::process::Command::new("sh")
            .args(["-c", "exec -a fcdev sleep 30 || sleep 30"])
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        write_pid_file(&path, child.id()).unwrap();
        let cmd = process_command(child.id());
        let result = claim_pid_file(&path);
        let _ = child.kill();
        let _ = child.wait();
        if cmd.as_deref().is_some_and(|c| c.contains("fcdev")) {
            let err = result.err().expect("must refuse").to_string();
            assert!(err.contains("already running"), "{err}");
        } else {
            // `exec -a` unsupported by this sh: the holder is `sleep`, a
            // reused PID from fc-dev's point of view, so it is replaced.
            assert!(result.is_ok());
        }
    }

    #[test]
    fn postmaster_pid_yields_pid_and_port() {
        let pm = parse_postmaster_pid(
            "12345\n/data\n1727000000\n15432\n/tmp\nlocalhost\n  5432001  65536\nready\n",
        )
        .unwrap();
        assert_eq!(
            pm,
            RunningPostmaster {
                pid: 12345,
                port: Some(15432)
            }
        );
        assert_eq!(
            parse_postmaster_pid("77\n"),
            Some(RunningPostmaster {
                pid: 77,
                port: None
            })
        );
        assert_eq!(parse_postmaster_pid("garbage"), None);
    }

    // The probe packs macOS's `struct flock` layout.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_cluster_lock_is_exclusive_across_processes() {
        let dir = tempfile::tempdir().unwrap();
        let _held = lock_cluster(dir.path()).unwrap();
        // A second process (POSIX locks are per process) must be refused:
        // `lockf`/python may be absent, so probe with perl's fcntl if present.
        let script = format!(
            "use Fcntl; open(my $f, '+<', '{}') or exit 3; \
             my $fl = pack('q q l s s', 0, 0, 0, F_WRLCK, SEEK_SET); \
             exit(fcntl($f, F_SETLK, $fl) ? 0 : 1);",
            dir.path().join("epg-lock").display()
        );
        if let Ok(status) = std::process::Command::new("perl")
            .args(["-e", &script])
            .status()
        {
            // 1 = refused; 0 would mean the lock is not held.
            assert_eq!(status.code(), Some(1), "second process got the lock");
        }
    }
}
