//! `fn build [<dir>]`: `cargo build --release --target wasm32-wasip2` in
//! the function's directory, then the component's path. Without cargo or
//! the `wasm32-wasip2` target it prints how to install them instead.
//! (Rust-only: Java's fcdev leaves the build to Maven.)

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

use super::{print_json, CliError, Ctx, Io, OutputMode};

pub const TARGET: &str = "wasm32-wasip2";

#[derive(clap::Args, Debug)]
pub struct BuildArgs {
    /// The function's directory (its Cargo.toml).
    #[arg(default_value = ".")]
    pub dir: PathBuf,
}

pub fn run(ctx: &Ctx<'_>, args: &BuildArgs, io: &mut Io<'_>) -> Result<i32, CliError> {
    if !args.dir.join("Cargo.toml").is_file() {
        return Err(CliError::Other(format!(
            "no Cargo.toml in {}: run fc-dev fn build in a function's directory (fc-dev fn init \
             makes one)",
            args.dir.display()
        )));
    }
    if Command::new("cargo")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| !s.success())
        .unwrap_or(true)
    {
        return Err(CliError::Other(
            "cargo was not found: install Rust from https://rustup.rs, then \
             `rustup target add wasm32-wasip2`"
                .into(),
        ));
    }
    // With rustup, check the target up front; without it (a distro
    // toolchain) let cargo say what is missing.
    if let Ok(output) = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .current_dir(&args.dir)
        .output()
    {
        if output.status.success()
            && !String::from_utf8_lossy(&output.stdout)
                .lines()
                .any(|l| l.trim() == TARGET)
        {
            return Err(CliError::Other(format!(
                "the {TARGET} target is not installed: run `rustup target add {TARGET}`"
            )));
        }
    }

    // Diagnostics go to stderr, as cargo renders them; the JSON messages on
    // stdout name the component.
    let mut child = Command::new("cargo")
        .args([
            "build",
            "--release",
            "--target",
            TARGET,
            "--message-format=json-render-diagnostics",
        ])
        .current_dir(&args.dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| CliError::Other(format!("could not run cargo: {e}")))?;
    let mut components = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            components.extend(component_paths(&message));
        }
    }
    let status = child.wait()?;
    if !status.success() {
        return Err(CliError::Other("cargo build failed".into()));
    }
    let Some(component) = components.last() else {
        return Err(CliError::Other(
            "cargo built no .wasm: is the crate a `cdylib` (see the template's Cargo.toml)?".into(),
        ));
    };
    match ctx.output() {
        OutputMode::Json => print_json(io.out, &json!({"artifact": component}))?,
        OutputMode::Text => {
            writeln!(io.out, "built {component}")?;
            writeln!(
                io.out,
                "next: fc-dev fn deploy {component} <app.service.name> --manifest manifest.json"
            )?;
        }
    }
    Ok(0)
}

/// The `.wasm` files a `compiler-artifact` message names for a `cdylib`.
fn component_paths(message: &Value) -> Vec<String> {
    if message["reason"] != "compiler-artifact" {
        return Vec::new();
    }
    let cdylib = message["target"]["kind"]
        .as_array()
        .is_some_and(|kinds| kinds.iter().any(|k| k == "cdylib"));
    if !cdylib {
        return Vec::new();
    }
    message["filenames"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|f| f.ends_with(".wasm"))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_component_is_the_cdylibs_wasm() {
        let message = json!({
            "reason": "compiler-artifact",
            "target": {"kind": ["cdylib"], "name": "hello"},
            "filenames": ["/p/target/wasm32-wasip2/release/hello.wasm"],
        });
        assert_eq!(
            component_paths(&message),
            vec!["/p/target/wasm32-wasip2/release/hello.wasm"]
        );
        let dep = json!({
            "reason": "compiler-artifact",
            "target": {"kind": ["lib"]},
            "filenames": ["/p/target/wasm32-wasip2/release/deps/libserde.rlib"],
        });
        assert!(component_paths(&dep).is_empty());
    }
}
