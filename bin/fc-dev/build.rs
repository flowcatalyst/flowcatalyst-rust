//! Build script: ensure the frontend is rebuilt before fc-dev is linked.
//!
//! `rust-embed` pulls `frontend/dist/` into the compiled binary at macro
//! expansion time. Without this script, `cargo build -p fc-dev` would
//! happily embed a stale dist — or fail if the dist doesn't exist at all.
//!
//! The script runs `pnpm build` (or `npm run build` as fallback) in the
//! frontend directory. It only reruns when frontend sources change, so
//! incremental Rust rebuilds stay fast.
//!
//! Escape hatch: set `FC_SKIP_FRONTEND_BUILD=1` to skip entirely — useful
//! when the frontend has already been built in a prior CI step and you
//! just want to link the Rust binary against the existing `dist/`.
#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a build script reports failure by panicking, which fails the build with the message"
)]

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    // Re-run the frontend build if any of these change. We avoid watching
    // `frontend/dist/` (our own output) and `frontend/node_modules/`.
    println!("cargo:rerun-if-env-changed=FC_SKIP_FRONTEND_BUILD");

    let crate_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let frontend_dir = crate_dir
        .join("../../frontend")
        .canonicalize()
        .expect("frontend/ directory not found relative to bin/fc-dev/");

    for watch in &[
        "src",
        "public",
        "index.html",
        "package.json",
        "pnpm-lock.yaml",
        "vite.config.ts",
        "tsconfig.json",
        "openapi-ts.config.ts",
    ] {
        let p = frontend_dir.join(watch);
        println!("cargo:rerun-if-changed={}", p.display());
    }

    if env::var("FC_SKIP_FRONTEND_BUILD").is_ok() {
        println!("cargo:warning=FC_SKIP_FRONTEND_BUILD set — skipping frontend build");
        return;
    }

    // Pick a package manager: pnpm preferred, npm as fallback.
    let pm = if which("pnpm").is_some() {
        "pnpm"
    } else if which("npm").is_some() {
        println!("cargo:warning=pnpm not found, falling back to npm (slower, may produce different node_modules)");
        "npm"
    } else {
        panic!(
            "Neither pnpm nor npm found on PATH. Install one (e.g. `brew install pnpm`), \
             or pre-build the frontend and set FC_SKIP_FRONTEND_BUILD=1."
        );
    };

    // If node_modules doesn't exist, install first.
    if !frontend_dir.join("node_modules").exists() {
        println!(
            "cargo:warning=Installing frontend dependencies with {} (first build)...",
            pm
        );
        let status = Command::new(pm)
            .arg("install")
            .current_dir(&frontend_dir)
            .status()
            .unwrap_or_else(|e| panic!("failed to run `{} install`: {}", pm, e));
        if !status.success() {
            panic!("`{} install` failed in frontend/", pm);
        }
    }

    // Run the actual build. `pnpm build` / `npm run build` both work.
    let status = Command::new(pm)
        .args(["run", "build"])
        .current_dir(&frontend_dir)
        .status()
        .unwrap_or_else(|e| panic!("failed to run `{} run build`: {}", pm, e));
    if !status.success() {
        panic!(
            "Frontend build failed. Run `cd frontend && {} build` to see the error, \
             or set FC_SKIP_FRONTEND_BUILD=1 to bypass.",
            pm
        );
    }

    let dist_dir = frontend_dir.join("dist");
    if !dist_dir.exists() {
        panic!(
            "Frontend build reported success but `{}` doesn't exist",
            dist_dir.display()
        );
    }
}

/// Tiny PATH-based `which` so we don't need a dep in the build script.
fn which(binary: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(binary);
        if candidate.is_file() {
            return Some(candidate);
        }
        // Windows: also check .cmd / .exe
        #[cfg(windows)]
        for ext in ["cmd", "exe", "bat"] {
            let with_ext = dir.join(format!("{}.{}", binary, ext));
            if with_ext.is_file() {
                return Some(with_ext);
            }
        }
    }
    None
}
