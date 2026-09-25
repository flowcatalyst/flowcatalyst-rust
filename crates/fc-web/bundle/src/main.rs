//! Write the Topcoat asset bundle for an already-built binary.
//!
//! `topcoat asset bundle` builds the binary itself and has no `--features`
//! flag, so it can't produce fc-dev's bundle (fc-web is behind fc-dev's
//! `web` feature). This runs the same bundler over the executable you point
//! it at:
//!
//! ```sh
//! cargo build -p fc-dev --features web
//! cargo run -p fc-web-bundle -- target/debug/fc-dev
//! ```
//!
//! The bundle lands in `assets/` next to the executable, where
//! `fc_web::service` looks for it (or pass an output directory).

use std::path::PathBuf;

fn main() {
    let mut args = std::env::args_os().skip(1);
    let Some(exe) = args.next().map(PathBuf::from) else {
        eprintln!("usage: fc-web-bundle <executable> [out-dir]");
        std::process::exit(2);
    };
    let out = args.next().map(PathBuf::from).unwrap_or_else(|| {
        exe.parent()
            .expect("executable has a parent directory")
            .join("assets")
    });
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target"));
    let cache = target_dir.join("topcoat").join("cache").join("assets");

    let bytes = std::fs::read(&exe).unwrap_or_else(|e| {
        eprintln!("cannot read {}: {e}", exe.display());
        std::process::exit(1);
    });
    let config = topcoat_asset::BundlerConfig::new().cache_dir(cache);
    if let Err(e) = topcoat_asset::Bundler::new(&config).bundle(&bytes, &out) {
        eprintln!("bundling failed: {e}");
        std::process::exit(1);
    }
    println!("bundled assets into {}", out.display());
}
