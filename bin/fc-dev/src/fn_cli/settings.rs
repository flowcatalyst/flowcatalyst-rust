//! `fn config get|set` and `fn secret set` (Java `ConfigCommand`,
//! `SecretCommand`). `config set` is a read-modify-write of the whole map.
//! A secret's value comes from stdin or `--from-file`, never an argument
//! (shell history), and is never printed. Both `set`s create the function
//! when its address is unknown, from `--manifest` (default `manifest.json`
//! when present), as `fn publish` does.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::deploy::{ensure_function_exists, read_manifest};
use super::{print_json, CliError, Ctx, Io, OutputMode};

#[derive(clap::Subcommand, Debug)]
pub enum ConfigCommand {
    /// Print the function's config values, and the declared keys still missing.
    Get {
        /// Full function address, app.service.name.
        address: String,
    },
    /// Set config values (the others are kept).
    Set {
        /// Full function address, app.service.name.
        address: String,
        /// KEY=VALUE pairs.
        #[arg(value_name = "KEY=VALUE", required = true)]
        values: Vec<String>,
        #[command(flatten)]
        create: CreateOpts,
    },
}

#[derive(clap::Subcommand, Debug)]
pub enum SecretCommand {
    /// Set a secret; the value is read from stdin (or --from-file).
    Set {
        /// Full function address, app.service.name.
        address: String,
        /// The secret's key.
        key: String,
        /// Read the value from this file instead of stdin.
        #[arg(long, value_name = "FILE")]
        from_file: Option<PathBuf>,
        #[command(flatten)]
        create: CreateOpts,
    },
}

#[derive(clap::Args, Debug)]
pub struct CreateOpts {
    /// The manifest to create the function from if it does not exist yet
    /// (default: manifest.json, when present).
    #[arg(long, value_name = "FILE")]
    pub manifest: Option<PathBuf>,
    /// Owning client id, when creating a client-owned function.
    #[arg(long, value_name = "ID")]
    pub client: Option<String>,
    /// Fail instead of creating the function when its address is unknown.
    #[arg(long)]
    pub no_create: bool,
}

impl CreateOpts {
    fn manifest(&self) -> Result<Option<Value>, CliError> {
        match &self.manifest {
            Some(path) => read_manifest(path).map(Some),
            None if Path::new("manifest.json").exists() => {
                read_manifest(Path::new("manifest.json")).map(Some)
            }
            None => Ok(None),
        }
    }
}

fn check_address(address: &str) -> Result<(), CliError> {
    if address.split('.').count() == 3 {
        Ok(())
    } else {
        Err(CliError::Usage(format!(
            "address must be app.service.name (three '.'-separated parts), got \"{address}\""
        )))
    }
}

pub async fn config(
    ctx: &Ctx<'_>,
    command: &ConfigCommand,
    io: &mut Io<'_>,
) -> Result<i32, CliError> {
    match command {
        ConfigCommand::Get { address } => {
            check_address(address)?;
            let client = ctx.client()?;
            let config = client
                .get(&format!("/api/functions/{address}/config"))
                .await?
                .unwrap_or(Value::Null);
            match ctx.output() {
                OutputMode::Json => print_json(io.out, &config)?,
                OutputMode::Text => {
                    for (k, v) in config["values"].as_object().into_iter().flatten() {
                        writeln!(io.out, "{k}={}", v.as_str().unwrap_or_default())?;
                    }
                    for missing in config["missing"].as_array().into_iter().flatten() {
                        writeln!(
                            io.out,
                            "# missing: {}",
                            missing.as_str().unwrap_or_default()
                        )?;
                    }
                }
            }
            Ok(0)
        }
        ConfigCommand::Set {
            address,
            values,
            create,
        } => {
            check_address(address)?;
            let mut updates = BTreeMap::new();
            for pair in values {
                match pair.split_once('=') {
                    Some((k, v)) if !k.trim().is_empty() => {
                        updates.insert(k.trim().to_string(), v.to_string());
                    }
                    _ => {
                        return Err(CliError::Usage(format!(
                            "expected KEY=VALUE, got \"{pair}\""
                        )))
                    }
                }
            }
            let client = ctx.client()?;
            let manifest = create.manifest()?;
            ensure_function_exists(
                &client,
                address,
                manifest.as_ref(),
                create.client.as_deref(),
                create.no_create,
            )
            .await?;
            let current = client
                .get(&format!("/api/functions/{address}/config"))
                .await?
                .unwrap_or(Value::Null);
            let mut merged: BTreeMap<String, String> = current["values"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                .collect();
            merged.extend(updates);
            let answer = client
                .put(
                    &format!("/api/functions/{address}/config"),
                    json!({"values": merged}),
                )
                .await?;
            match ctx.output() {
                OutputMode::Json => {
                    print_json(io.out, &answer.unwrap_or(json!({"values": merged})))?
                }
                OutputMode::Text => writeln!(io.out, "{address}: config updated")?,
            }
            Ok(0)
        }
    }
}

pub async fn secret(
    ctx: &Ctx<'_>,
    command: &SecretCommand,
    io: &mut Io<'_>,
) -> Result<i32, CliError> {
    match command {
        SecretCommand::Set {
            address,
            key,
            from_file,
            create,
        } => {
            check_address(address)?;
            let raw = match from_file {
                Some(path) => std::fs::read_to_string(path).map_err(|e| {
                    CliError::Other(format!("could not read {}: {e}", path.display()))
                })?,
                None => {
                    let mut buf = String::new();
                    io.stdin.read_to_string(&mut buf)?;
                    buf
                }
            };
            // A trailing newline from `echo` or a file is not part of the value.
            let value = raw.strip_suffix('\n').unwrap_or(&raw);
            let value = value.strip_suffix('\r').unwrap_or(value);
            if value.is_empty() {
                return Err(CliError::Usage(
                    "the secret's value is empty (pipe it on stdin, or pass --from-file)".into(),
                ));
            }
            let client = ctx.client()?;
            let manifest = create.manifest()?;
            ensure_function_exists(
                &client,
                address,
                manifest.as_ref(),
                create.client.as_deref(),
                create.no_create,
            )
            .await?;
            client
                .put(
                    &format!("/api/functions/{address}/secrets/{key}"),
                    json!({"value": value}),
                )
                .await?;
            match ctx.output() {
                OutputMode::Json => print_json(io.out, &json!({"address": address, "key": key}))?,
                OutputMode::Text => writeln!(io.out, "{address}: secret {key} set")?,
            }
            Ok(0)
        }
    }
}
