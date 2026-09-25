//! `fn invoke <address>[:<version>]` (Java `InvokeCommand`): calls the
//! function host, never the platform, at
//! `<host>/functions/<address>[:<version>]<path>`. The host is `--host-url`,
//! else `fn-cli.json`'s `hostUrl`, else `http://127.0.0.1:8090`. A versioned
//! call carries the CLI's own bearer token. `--webhook` signs the request
//! as the platform signs a delivery, with `--signing-secret`. Exit 0 when
//! the status is below 400.

use std::path::PathBuf;

use serde_json::json;

use super::client::{bearer_header, raw};
use super::credentials::{trim_slash, CliFile};
use super::{print_json, CliError, Ctx, Io, OutputMode};

const DEFAULT_HOST_URL: &str = "http://127.0.0.1:8090";

#[derive(clap::Args, Debug)]
pub struct InvokeArgs {
    /// Full function address, optionally suffixed :<version> for a
    /// versioned call.
    #[arg(value_name = "ADDRESS[:VERSION]")]
    pub address: String,

    /// The function path (default: none, i.e. /).
    #[arg(long, default_value = "")]
    pub path: String,

    /// HTTP method.
    #[arg(long, default_value = "GET")]
    pub method: String,

    /// Request body file, or - for stdin.
    #[arg(long, value_name = "FILE|-")]
    pub body: Option<String>,

    /// An extra request header, k:v (repeatable).
    #[arg(short = 'H', value_name = "K:V")]
    pub header: Vec<String>,

    /// The function host's base URL.
    #[arg(long, value_name = "URL")]
    pub host_url: Option<String>,

    /// Sign the request as the platform would for a webhook endpoint
    /// (requires --signing-secret).
    #[arg(long)]
    pub webhook: bool,

    /// The application's webhook signing secret (with --webhook).
    #[arg(long, value_name = "SECRET")]
    pub signing_secret: Option<String>,
}

pub async fn run(ctx: &Ctx<'_>, args: &InvokeArgs, io: &mut Io<'_>) -> Result<i32, CliError> {
    let (address, version) = split_version(&args.address);
    if address.split('.').count() != 3 {
        return Err(CliError::Usage(format!(
            "address must be app.service.name (three '.'-separated parts), got \"{address}\""
        )));
    }
    let signing_secret = args.signing_secret.as_deref().filter(|s| !s.is_empty());
    if args.webhook && signing_secret.is_none() {
        return Err(CliError::Usage(
            "--webhook requires --signing-secret (no platform route returns an application's \
             signing secret to a caller)"
                .into(),
        ));
    }
    let body = match args.body.as_deref() {
        None | Some("") => Vec::new(),
        Some("-") => {
            let mut buf = Vec::new();
            io.stdin.read_to_end(&mut buf)?;
            buf
        }
        Some(file) => std::fs::read(PathBuf::from(file))
            .map_err(|e| CliError::Other(format!("could not read {file}: {e}")))?,
    };
    let mut headers = Vec::new();
    for h in &args.header {
        match h.split_once(':') {
            Some((k, v)) if !k.trim().is_empty() => {
                headers.push((k.trim().to_string(), v.trim().to_string()))
            }
            _ => return Err(CliError::Usage(format!("expected -H k:v, got \"{h}\""))),
        }
    }

    let host = args
        .host_url
        .clone()
        .filter(|u| !u.trim().is_empty())
        .or_else(|| CliFile::read(&ctx.credentials_file()).and_then(|f| f.host_url))
        .map(trim_slash)
        .unwrap_or_else(|| DEFAULT_HOST_URL.to_string());
    let url = match version {
        Some(v) => format!("{host}/functions/{address}:{v}{}", args.path),
        None => format!("{host}/functions/{address}{}", args.path),
    };
    if version.is_some() {
        let token = ctx.client()?.bearer_token().await?;
        headers.push(bearer_header(&token));
    }
    if let Some(secret) = signing_secret.filter(|_| args.webhook) {
        for (name, value) in fc_platform::shared::webhook_signer::signature_headers(
            secret,
            chrono::Utc::now(),
            &body,
        ) {
            headers.push((name.to_string(), value));
        }
    }

    let response = raw(&args.method, &url, &headers, body).await?;
    match ctx.output() {
        OutputMode::Json => {
            let headers: serde_json::Map<String, serde_json::Value> = response
                .headers
                .iter()
                .map(|(k, v)| (k.clone(), json!(v)))
                .collect();
            print_json(
                io.out,
                &json!({"status": response.status, "headers": headers, "body": response.body_text()}),
            )?;
        }
        OutputMode::Text => {
            writeln!(io.out, "HTTP {}", response.status)?;
            for (k, v) in &response.headers {
                writeln!(io.out, "{k}: {v}")?;
            }
            writeln!(io.out)?;
            writeln!(io.out, "{}", response.body_text())?;
        }
    }
    Ok(if response.status < 400 { 0 } else { 1 })
}

/// `app.service.name:7` → (`app.service.name`, `Some(7)`); a suffix that is
/// not all digits stays part of the address.
fn split_version(raw: &str) -> (&str, Option<u32>) {
    if let Some((address, suffix)) = raw.rsplit_once(':') {
        if !address.is_empty() && !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()) {
            if let Ok(v) = suffix.parse() {
                return (address, Some(v));
            }
        }
    }
    (raw, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_numeric_suffix_is_the_version() {
        assert_eq!(split_version("a.b.c:3"), ("a.b.c", Some(3)));
        assert_eq!(split_version("a.b.c"), ("a.b.c", None));
        assert_eq!(split_version("a.b.c:live"), ("a.b.c:live", None));
    }
}
