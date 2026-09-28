//! The model is pure (no database, no async runtime, no HTTP) so a WASM
//! guest can link it: it must build for `wasm32-unknown-unknown`. Skipped
//! (with a note) when that target's standard library is not installed. Uses
//! its own target directory, so it never waits on the lock of the build
//! running this test.

use std::path::Path;
use std::process::Command;

const TARGET: &str = "wasm32-unknown-unknown";

fn target_installed() -> bool {
    Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .is_some_and(|sysroot| {
            Path::new(sysroot.trim())
                .join("lib/rustlib")
                .join(TARGET)
                .exists()
        })
}

#[test]
fn builds_for_wasm32_unknown_unknown() {
    if !target_installed() {
        eprintln!("skipped: the {TARGET} target is not installed (rustup target add {TARGET})");
        return;
    }
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let target_dir = manifest_dir.join("../../target/wasm32-check");
    let output = Command::new(env!("CARGO"))
        .current_dir(manifest_dir)
        .args([
            "build",
            "--quiet",
            "--lib",
            "--target",
            TARGET,
            "-p",
            "fc-function-model",
        ])
        .arg("--target-dir")
        .arg(&target_dir)
        .output()
        .expect("run cargo");
    assert!(
        output.status.success(),
        "cargo build --target {TARGET} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
