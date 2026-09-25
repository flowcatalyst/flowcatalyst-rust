//! `fn publish`, `fn deploy` and `fn promote` (Java `PublishCommand`,
//! `Publisher`, `DeployCommand`, `PromoteCommand`, `Promoter`).
//!
//! Publish: the artifact's sha256; the function is created when its
//! address is unknown (runtime from the manifest, `--client` for a
//! client-owned one; `--no-create` forbids it); the artifact is uploaded
//! (`PUT …/artifacts/{digest}`) and the `platform://` ref the platform
//! answers is what is published (`POST …/versions`). `--artifact-ref`
//! (`oci://` / `s3://`) publishes by reference instead.
//!
//! Deploy: publish, then promote `live`. Deploying the same artifact twice
//! succeeds: the platform answers a republish of the same digest and
//! manifest `200` with the existing version, and a promote to the version
//! the alias already names `200` with `changed: false`. An older platform
//! answered both `409` (`VERSION_DIGEST_EXISTS` naming the version,
//! `ALIAS_UNCHANGED`); those answers are still taken the same way.
//!
//! Promote: polls `…/status` once a second until the version is `READY`
//! (`--wait 0` skips the wait), then `PUT …/aliases/{alias}`. A timeout
//! prints each host's state for the version and exits 1.
//!
//! Both promote optimistically: the alias's version is read before the
//! wait (or given with `--expected-version`) and sent as `If-Match`, so a
//! promote made by someone else meanwhile is `412 ALIAS_VERSION_CONFLICT`
//! rather than silently overwritten. A header, not the body's
//! `expectedVersion`, so an older platform simply ignores it.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::client::FnClient;
use super::{parse_duration, print_json, AddressOpts, CliError, Ctx, Io, OutputMode};

#[derive(clap::Args, Debug)]
pub struct PublishArgs {
    /// The function's artifact: a WASM component (`.wasm`).
    pub artifact: PathBuf,

    /// Full function address, app.service.name.
    pub address: Option<String>,

    /// The manifest JSON file.
    #[arg(long, value_name = "FILE", default_value = "manifest.json")]
    pub manifest: PathBuf,

    /// oci://… or s3://… (omit to upload through the platform).
    #[arg(long, value_name = "REF")]
    pub artifact_ref: Option<String>,

    /// Sigstore bundle file (cosign sign-blob of the artifact).
    #[arg(long, value_name = "FILE")]
    pub bundle: Option<PathBuf>,

    /// Owning client id, when creating a client-owned function.
    #[arg(long, value_name = "ID")]
    pub client: Option<String>,

    /// Fail instead of creating the function when its address is unknown.
    #[arg(long)]
    pub no_create: bool,

    #[command(flatten)]
    pub address_opts: AddressOpts,
}

#[derive(clap::Args, Debug)]
pub struct DeployArgs {
    #[command(flatten)]
    pub publish: PublishArgs,

    /// How long to wait for READY before giving up (0 promotes at once).
    #[arg(long, value_name = "DURATION", default_value = "60s", value_parser = parse_duration)]
    pub wait: Duration,
}

#[derive(clap::Args, Debug)]
pub struct PromoteArgs {
    /// Full function address, app.service.name.
    pub address: Option<String>,

    /// The version to promote.
    #[arg(long)]
    pub version: i64,

    /// The alias to point at it.
    #[arg(long, default_value = "live")]
    pub alias: String,

    /// The version the alias must point at now (0: none yet); default, the
    /// one it points at when the command starts.
    #[arg(long, value_name = "N")]
    pub expected_version: Option<i64>,

    /// How long to wait for READY before giving up (0 promotes at once).
    #[arg(long, value_name = "DURATION", default_value = "60s", value_parser = parse_duration)]
    pub wait: Duration,

    #[command(flatten)]
    pub address_opts: AddressOpts,
}

struct Published {
    address: String,
    version: i64,
    digest: String,
}

pub async fn publish(ctx: &Ctx<'_>, args: &PublishArgs, io: &mut Io<'_>) -> Result<i32, CliError> {
    let address = args.address_opts.resolve(args.address.as_deref())?;
    let client = ctx.client()?;
    let published = publish_version(&client, &address, args).await?;
    match ctx.output() {
        OutputMode::Text => writeln!(
            io.out,
            "published {} version {} ({})",
            published.address, published.version, published.digest
        )?,
        OutputMode::Json => print_json(
            io.out,
            &json!({"address": published.address, "version": published.version, "digest": published.digest}),
        )?,
    }
    Ok(0)
}

pub async fn deploy(ctx: &Ctx<'_>, args: &DeployArgs, io: &mut Io<'_>) -> Result<i32, CliError> {
    let address = args
        .publish
        .address_opts
        .resolve(args.publish.address.as_deref())?;
    let client = ctx.client()?;
    let version = match publish_version(&client, &address, &args.publish).await {
        Ok(published) => published.version,
        Err(e) => older_platform_existing_version(&e).ok_or(e)?,
    };
    let expected = alias_version(&client, &address, "live").await?;
    let promoted = match promote_when_ready(
        ctx, &client, &address, "live", version, expected, args.wait, io,
    )
    .await
    {
        Err(e) if is_older_platform_unchanged_alias(&e) => {
            json!({"alias": "live", "version": version, "changed": false})
        }
        other => other?,
    };
    match ctx.output() {
        OutputMode::Text => {
            let version = promoted["version"].as_i64().unwrap_or(version);
            if promoted["changed"] == Value::Bool(false) {
                writeln!(io.out, "{address}: version {version} is already live")?
            } else {
                writeln!(io.out, "{address}: version {version} deployed and live")?
            }
        }
        OutputMode::Json => print_json(io.out, &promoted)?,
    }
    Ok(0)
}

/// An older platform's `409 VERSION_DIGEST_EXISTS`: the version it names,
/// which a current platform answers `200` with instead.
fn older_platform_existing_version(e: &CliError) -> Option<i64> {
    match e {
        CliError::Platform { code, details, .. } if code == "VERSION_DIGEST_EXISTS" => {
            details["version"].as_i64()
        }
        _ => None,
    }
}

/// An older platform's `409 ALIAS_UNCHANGED`, which a current platform
/// answers `200` with `changed: false` instead.
fn is_older_platform_unchanged_alias(e: &CliError) -> bool {
    e.code() == Some("ALIAS_UNCHANGED")
}

pub async fn promote(ctx: &Ctx<'_>, args: &PromoteArgs, io: &mut Io<'_>) -> Result<i32, CliError> {
    let address = args.address_opts.resolve(args.address.as_deref())?;
    let client = ctx.client()?;
    let expected = match args.expected_version {
        Some(v) => v,
        None => alias_version(&client, &address, &args.alias).await?,
    };
    let promoted = promote_when_ready(
        ctx,
        &client,
        &address,
        &args.alias,
        args.version,
        expected,
        args.wait,
        io,
    )
    .await?;
    match ctx.output() {
        OutputMode::Text => writeln!(
            io.out,
            "{address}: {} -> version {}",
            args.alias, args.version
        )?,
        OutputMode::Json => print_json(io.out, &promoted)?,
    }
    Ok(0)
}

async fn publish_version(
    client: &FnClient,
    address: &str,
    args: &PublishArgs,
) -> Result<Published, CliError> {
    let bytes = std::fs::read(&args.artifact)
        .map_err(|e| CliError::Other(format!("could not read {}: {e}", args.artifact.display())))?;
    let digest = format!("sha256:{}", hex::encode(Sha256::digest(&bytes)));
    let manifest = read_manifest(&args.manifest)?;

    // The function must exist before the upload: the route 404s otherwise.
    ensure_function_exists(
        client,
        address,
        Some(&manifest),
        args.client.as_deref(),
        args.no_create,
    )
    .await?;
    let artifact_ref = match args
        .artifact_ref
        .as_deref()
        .filter(|r| !r.trim().is_empty())
    {
        Some(r) if r.starts_with("oci://") || r.starts_with("s3://") => r.to_string(),
        Some(r) => {
            return Err(CliError::Usage(format!(
                "--artifact-ref must start with oci:// or s3://, got \"{r}\""
            )))
        }
        None => {
            let uploaded = client
                .put_bytes(
                    &format!("/api/functions/{address}/artifacts/{digest}"),
                    bytes,
                )
                .await?
                .unwrap_or(Value::Null);
            uploaded["artifactRef"]
                .as_str()
                .ok_or_else(|| CliError::Other("the upload answered no artifactRef".into()))?
                .to_string()
        }
    };
    let mut body = json!({"artifactRef": artifact_ref, "digest": digest, "manifest": manifest});
    if let Some(bundle) = &args.bundle {
        let bundle = std::fs::read_to_string(bundle)
            .map_err(|e| CliError::Other(format!("could not read {}: {e}", bundle.display())))?;
        body["signatureBundle"] = Value::String(bundle);
    }
    let response = client
        .post(&format!("/api/functions/{address}/versions"), body)
        .await?
        .unwrap_or(Value::Null);
    Ok(Published {
        address: address.to_string(),
        version: response["version"]
            .as_i64()
            .ok_or_else(|| CliError::Other("the publish answered no version".into()))?,
        digest: response["digest"].as_str().unwrap_or(&digest).to_string(),
    })
}

/// A `GET` of the function and, on a 404 only, its creation from the
/// manifest's `runtime` (Java `Publisher.ensureFunctionExists`). Shared
/// with `config set` / `secret set`, whose manifest is optional.
pub async fn ensure_function_exists(
    client: &FnClient,
    address: &str,
    manifest: Option<&Value>,
    owner: Option<&str>,
    no_create: bool,
) -> Result<(), CliError> {
    match client.get(&format!("/api/functions/{address}")).await {
        Ok(_) => Ok(()),
        Err(e) if e.status() == Some(404) && !no_create => {
            let Some(manifest) = manifest else {
                return Err(CliError::Other(format!(
                    "function {address} does not exist and no manifest.json was found to create it \
                     from; pass --manifest, or run fn publish first"
                )));
            };
            let mut parts = address.splitn(3, '.');
            let (app, service, name) = (
                parts.next().unwrap_or_default(),
                parts.next().unwrap_or_default(),
                parts.next().unwrap_or_default(),
            );
            let mut create = json!({
                "applicationCode": app,
                "serviceName": service,
                "name": name,
                "runtime": manifest.get("runtime").cloned().unwrap_or(Value::Null),
            });
            if let Some(owner) = owner.filter(|o| !o.trim().is_empty()) {
                create["clientId"] = Value::String(owner.to_string());
            }
            client.post("/api/functions", create).await?;
            Ok(())
        }
        Err(e) => Err(e),
    }
}

pub fn read_manifest(path: &Path) -> Result<Value, CliError> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| CliError::Other(format!("could not read {}: {e}", path.display())))?;
    serde_json::from_str(&raw).map_err(|e| {
        CliError::Usage(format!(
            "invalid JSON in manifest file {}: {e}",
            path.display()
        ))
    })
}

/// The version `alias` points at now, `0` when it has none.
async fn alias_version(client: &FnClient, address: &str, alias: &str) -> Result<i64, CliError> {
    let aliases = client
        .get(&format!("/api/functions/{address}/aliases"))
        .await?
        .unwrap_or(Value::Null);
    Ok(version_of_alias(&aliases, alias))
}

fn version_of_alias(aliases: &Value, alias: &str) -> i64 {
    aliases
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["alias"] == alias)
        .and_then(|a| a["version"].as_i64())
        .unwrap_or(0)
}

#[allow(clippy::too_many_arguments)]
async fn promote_when_ready(
    ctx: &Ctx<'_>,
    client: &FnClient,
    address: &str,
    alias: &str,
    version: i64,
    expected_version: i64,
    wait: Duration,
    io: &mut Io<'_>,
) -> Result<Value, CliError> {
    if !wait.is_zero() {
        let deadline = Instant::now() + wait;
        loop {
            let status = client
                .get(&format!("/api/functions/{address}/status"))
                .await?
                .unwrap_or(Value::Null);
            if is_ready(&status, version) {
                break;
            }
            if Instant::now() >= deadline {
                print_timeout(ctx, io, address, version, &status)?;
                return Err(CliError::Reported);
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    Ok(client
        .put_with_headers(
            &format!("/api/functions/{address}/aliases/{alias}"),
            json!({"version": version}),
            &[("if-match", format!("\"{expected_version}\""))],
        )
        .await?
        .unwrap_or(Value::Null))
}

fn is_ready(status: &Value, version: i64) -> bool {
    status["versions"].as_array().is_some_and(|versions| {
        versions
            .iter()
            .any(|v| v["version"].as_i64() == Some(version) && v["state"] == "READY")
    })
}

fn print_timeout(
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
    address: &str,
    version: i64,
    status: &Value,
) -> Result<(), CliError> {
    let hosts: Vec<Value> = status["hosts"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|host| {
            host["loaded"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|l| l["version"].as_i64() == Some(version))
                .map(|l| {
                    json!({"hostId": host["hostId"], "state": l["state"], "error": l.get("error").cloned().unwrap_or(Value::Null)})
                })
                .collect::<Vec<_>>()
        })
        .collect();
    match ctx.output() {
        OutputMode::Json => writeln!(
            io.err,
            "{}",
            json!({"error": "TIMEOUT", "address": address, "version": version, "hosts": hosts})
        )?,
        OutputMode::Text => {
            writeln!(
                io.err,
                "timed out waiting for {address} version {version} to become READY:"
            )?;
            if hosts.is_empty() {
                writeln!(io.err, "  (no host reports this version)")?;
            }
            for h in &hosts {
                let error = h["error"]
                    .as_str()
                    .map(|e| format!(" ({e})"))
                    .unwrap_or_default();
                writeln!(
                    io.err,
                    "  {}: {}{error}",
                    h["hostId"].as_str().unwrap_or("?"),
                    h["state"].as_str().unwrap_or("?")
                )?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_older_platforms_no_op_conflicts_are_still_taken_as_success() {
        let dup = CliError::Platform {
            code: "VERSION_DIGEST_EXISTS".into(),
            message: "dup".into(),
            status: 409,
            details: json!({"version": 4}),
        };
        assert_eq!(older_platform_existing_version(&dup), Some(4));
        let other = CliError::Platform {
            code: "FUNCTION_DISABLED".into(),
            message: "x".into(),
            status: 409,
            details: Value::Null,
        };
        assert_eq!(older_platform_existing_version(&other), None);
        let unchanged = CliError::Platform {
            code: "ALIAS_UNCHANGED".into(),
            message: "x".into(),
            status: 409,
            details: Value::Null,
        };
        assert!(is_older_platform_unchanged_alias(&unchanged));
        assert!(!is_older_platform_unchanged_alias(&other));
    }

    #[test]
    fn the_expected_version_is_the_aliases_or_zero() {
        let aliases = json!([{"alias": "live", "version": 3}, {"alias": "qa", "version": 1}]);
        assert_eq!(version_of_alias(&aliases, "live"), 3);
        assert_eq!(version_of_alias(&aliases, "qa"), 1);
        assert_eq!(version_of_alias(&aliases, "canary"), 0);
        assert_eq!(version_of_alias(&Value::Null, "live"), 0);
    }

    #[test]
    fn ready_is_the_version_in_state_ready() {
        let status = json!({"versions": [{"version": 1, "state": "READY"}, {"version": 2, "state": "PUBLISHED"}]});
        assert!(is_ready(&status, 1));
        assert!(!is_ready(&status, 2));
        assert!(!is_ready(&status, 3));
    }
}
