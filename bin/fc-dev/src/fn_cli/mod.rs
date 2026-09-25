//! `fc-dev fn`: the developer surface for functions (plan H8; Java `fcdev
//! fn`, `docs/spec/function-developer-surface.md` §2 in the Java repo).
//!
//! | Command | Does |
//! |---|---|
//! | `init <dir>` | scaffolds a Rust WASM function from `templates/function-rust` (local only) |
//! | `build [<dir>]` | `cargo build --release --target wasm32-wasip2`, and prints the component's path |
//! | `publish <artifact> [<address>]` | uploads the artifact, then publishes a version with the manifest; creates the function when its address is unknown |
//! | `deploy <artifact> [<address>]` | publish, wait for `READY`, promote `live` |
//! | `promote [<address>] --version <n>` | wait for `READY`, promote an alias |
//! | `invoke <address>[:<version>]` | calls the function through the host |
//! | `config get\|set`, `secret set` | the function's platform-side settings |
//!
//! Global options (accepted after any subcommand, too): `--platform-url`,
//! `--client-id`, `--client-secret`, `--output text|json`. Exit codes: 0 ok;
//! 1 a platform or validation error (the platform's code and message, one
//! line); 2 a usage error.

mod build;
pub mod client;
pub mod credentials;
mod deploy;
mod init;
mod invoke;
mod settings;

use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

use client::FnClient;
use credentials::{Credentials, Flags};

#[derive(clap::Args, Debug)]
#[command(
    about = "Manage functions: init, build, publish, deploy, promote, invoke, config, secret",
    subcommand_required = true,
    arg_required_else_help = true
)]
pub struct FnArgs {
    /// Platform base URL (FLOWCATALYST_PLATFORM_URL; else fn-cli.json's,
    /// written by a running fc-dev).
    #[arg(long, global = true, value_name = "URL")]
    pub platform_url: Option<String>,

    /// OAuth client id (FLOWCATALYST_CLIENT_ID; else fn-cli.json's).
    #[arg(long, global = true, value_name = "ID")]
    pub client_id: Option<String>,

    /// OAuth client secret (FLOWCATALYST_CLIENT_SECRET; else fn-cli.json's).
    #[arg(long, global = true, value_name = "SECRET")]
    pub client_secret: Option<String>,

    /// Output mode.
    #[arg(long, global = true, value_enum, default_value_t = OutputMode::Text)]
    pub output: OutputMode,

    /// The credentials file (default: fc-dev's data dir/fn-cli.json).
    #[arg(long, global = true, hide = true, value_name = "FILE")]
    pub credentials_file: Option<PathBuf>,

    #[command(subcommand)]
    pub command: FnCommand,
}

#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Text,
    Json,
}

#[derive(clap::Subcommand, Debug)]
pub enum FnCommand {
    /// Scaffold a function project (local only; never contacts the platform).
    Init(init::InitArgs),
    /// Build the function's WASM component (cargo, wasm32-wasip2).
    Build(build::BuildArgs),
    /// Publish a new version of a function.
    Publish(deploy::PublishArgs),
    /// Publish a new version and promote it to live.
    Deploy(deploy::DeployArgs),
    /// Promote a version to an alias once it is READY.
    Promote(deploy::PromoteArgs),
    /// Call a function through the function host.
    Invoke(invoke::InvokeArgs),
    /// A function's config values.
    #[command(subcommand)]
    Config(settings::ConfigCommand),
    /// A function's secrets.
    #[command(subcommand)]
    Secret(settings::SecretCommand),
}

/// A command's failure. `Usage` exits 2; everything else exits 1.
#[derive(Debug)]
pub enum CliError {
    Usage(String),
    Platform {
        code: String,
        message: String,
        status: u16,
        details: Value,
    },
    Other(String),
    /// Already printed; just exit 1.
    Reported,
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::Usage(m) | CliError::Other(m) => f.write_str(m),
            CliError::Platform { code, message, .. } if message.is_empty() => f.write_str(code),
            CliError::Platform { code, message, .. } => write!(f, "{code}: {message}"),
            CliError::Reported => f.write_str("failed"),
        }
    }
}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        CliError::Other(e.to_string())
    }
}

impl CliError {
    pub fn code(&self) -> Option<&str> {
        match self {
            CliError::Platform { code, .. } => Some(code),
            _ => None,
        }
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            CliError::Platform { status, .. } => Some(*status),
            _ => None,
        }
    }
}

/// Where a command writes, and reads stdin from.
pub struct Io<'a> {
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
    pub stdin: &'a mut dyn Read,
}

/// What every command shares: the global options and the environment.
pub struct Ctx<'a> {
    pub args: &'a FnArgs,
    pub env: &'a dyn Fn(&str) -> Option<String>,
}

impl Ctx<'_> {
    pub fn output(&self) -> OutputMode {
        self.args.output
    }

    pub fn credentials_file(&self) -> PathBuf {
        self.args
            .credentials_file
            .clone()
            .unwrap_or_else(crate::functions::cli_file_path)
    }

    pub fn credentials(&self) -> Result<Credentials, CliError> {
        credentials::resolve(
            &Flags {
                platform_url: self.args.platform_url.as_deref(),
                client_id: self.args.client_id.as_deref(),
                client_secret: self.args.client_secret.as_deref(),
            },
            self.env,
            &self.credentials_file(),
        )
    }

    pub fn client(&self) -> Result<FnClient, CliError> {
        Ok(FnClient::new(self.credentials()?))
    }
}

/// Runs `args` against the process environment, on the process's stdio.
/// Returns the exit code.
pub async fn run(args: FnArgs) -> i32 {
    let mut out = std::io::stdout();
    let mut err = std::io::stderr();
    let mut stdin = std::io::stdin();
    let env = |key: &str| std::env::var(key).ok();
    run_with(
        &args,
        &env,
        &mut Io {
            out: &mut out,
            err: &mut err,
            stdin: &mut stdin,
        },
    )
    .await
}

/// [`run`] with the environment and stdio given: the seam tests use.
pub async fn run_with(args: &FnArgs, env: &dyn Fn(&str) -> Option<String>, io: &mut Io<'_>) -> i32 {
    let ctx = Ctx { args, env };
    let result = match &args.command {
        FnCommand::Init(a) => init::run(&ctx, a, io),
        FnCommand::Build(a) => build::run(&ctx, a, io),
        FnCommand::Publish(a) => deploy::publish(&ctx, a, io).await,
        FnCommand::Deploy(a) => deploy::deploy(&ctx, a, io).await,
        FnCommand::Promote(a) => deploy::promote(&ctx, a, io).await,
        FnCommand::Invoke(a) => invoke::run(&ctx, a, io).await,
        FnCommand::Config(c) => settings::config(&ctx, c, io).await,
        FnCommand::Secret(c) => settings::secret(&ctx, c, io).await,
    };
    match result {
        Ok(code) => code,
        Err(CliError::Reported) => 1,
        Err(e @ CliError::Usage(_)) => {
            let _ = writeln!(io.err, "error: {e}");
            2
        }
        Err(e) => {
            let _ = writeln!(io.err, "{e}");
            1
        }
    }
}

/// A full function address, or `--app` / `--service` / `--name` (Java
/// `AddressOptions`). A two-part address is a usage error.
#[derive(clap::Args, Debug, Clone, Default)]
pub struct AddressOpts {
    /// Application code (with --name, instead of a full address).
    #[arg(long, value_name = "CODE")]
    pub app: Option<String>,
    /// Service name.
    #[arg(long, value_name = "NAME", default_value = "default")]
    pub service: String,
    /// Function name (with --app, instead of a full address).
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,
}

impl AddressOpts {
    pub fn resolve(&self, full: Option<&str>) -> Result<String, CliError> {
        let full = full.filter(|a| !a.trim().is_empty());
        let blank = |v: &Option<String>| v.as_deref().is_none_or(|v| v.trim().is_empty());
        let has_parts = !blank(&self.app) || !blank(&self.name);
        match full {
            Some(_) if has_parts => Err(CliError::Usage(
                "give either a full address or --app/--service/--name, not both".into(),
            )),
            Some(full) if full.split('.').count() != 3 => Err(CliError::Usage(format!(
                "address must be app.service.name (three '.'-separated parts), got \"{full}\""
            ))),
            Some(full) => Ok(full.to_string()),
            None if blank(&self.app) => Err(CliError::Usage(
                "an address is required: give app.service.name, or --app (with --name)".into(),
            )),
            None if blank(&self.name) => Err(CliError::Usage(
                "--name is required together with --app".into(),
            )),
            None => {
                let service = if self.service.trim().is_empty() {
                    "default"
                } else {
                    self.service.as_str()
                };
                Ok(format!(
                    "{}.{}.{}",
                    self.app.as_deref().unwrap_or_default(),
                    service,
                    self.name.as_deref().unwrap_or_default()
                ))
            }
        }
    }
}

/// `60s`, `2m`, `500ms`, `1h`, or a bare number of seconds (`0` promotes
/// without waiting).
pub fn parse_duration(raw: &str) -> Result<Duration, String> {
    let raw = raw.trim();
    let (number, unit) = match raw.find(|c: char| !c.is_ascii_digit()) {
        Some(i) => raw.split_at(i),
        None => (raw, "s"),
    };
    let n: u64 = number
        .parse()
        .map_err(|_| format!("not a duration: \"{raw}\" (e.g. 60s, 2m, 500ms)"))?;
    match unit {
        "ms" => Ok(Duration::from_millis(n)),
        "s" => Ok(Duration::from_secs(n)),
        "m" => Ok(Duration::from_secs(n * 60)),
        "h" => Ok(Duration::from_secs(n * 3600)),
        _ => Err(format!("not a duration: \"{raw}\" (e.g. 60s, 2m, 500ms)")),
    }
}

/// Prints `value` as one JSON line.
pub fn print_json(out: &mut dyn Write, value: &Value) -> Result<(), CliError> {
    writeln!(out, "{value}")?;
    Ok(())
}

/// Parses `fc-dev fn <args>`, for tests.
#[cfg(test)]
pub fn parse(args: &[&str]) -> Result<FnArgs, clap::Error> {
    use clap::Parser;

    #[derive(Parser)]
    struct Harness {
        #[command(subcommand)]
        command: Top,
    }

    #[derive(clap::Subcommand)]
    enum Top {
        Fn(FnArgs),
    }

    let mut all = vec!["fc-dev", "fn"];
    all.extend_from_slice(args);
    Harness::try_parse_from(all).map(|h| match h.command {
        Top::Fn(a) => a,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_options_are_accepted_after_the_subcommand() {
        let args = parse(&[
            "deploy",
            "a.wasm",
            "acme.default.hello",
            "--output",
            "json",
            "--client-id",
            "x",
        ])
        .unwrap();
        assert_eq!(args.output, OutputMode::Json);
        assert_eq!(args.client_id.as_deref(), Some("x"));
    }

    #[test]
    fn an_address_is_three_parts_or_app_and_name() {
        let opts = AddressOpts {
            service: "default".into(),
            ..Default::default()
        };
        assert_eq!(opts.resolve(Some("a.b.c")).unwrap(), "a.b.c");
        assert!(matches!(opts.resolve(Some("a.b")), Err(CliError::Usage(_))));
        assert!(matches!(opts.resolve(None), Err(CliError::Usage(_))));
        let parts = AddressOpts {
            app: Some("acme".into()),
            service: "default".into(),
            name: Some("hello".into()),
        };
        assert_eq!(parts.resolve(None).unwrap(), "acme.default.hello");
        assert!(matches!(
            parts.resolve(Some("a.b.c")),
            Err(CliError::Usage(_))
        ));
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration("60s").unwrap(), Duration::from_secs(60));
        assert_eq!(parse_duration("2m").unwrap(), Duration::from_secs(120));
        assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
        assert_eq!(parse_duration("0").unwrap(), Duration::ZERO);
        assert!(parse_duration("soon").is_err());
    }
}
