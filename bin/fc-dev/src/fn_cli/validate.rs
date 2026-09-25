//! `fn validate <address> --manifest <file> [--alias <name>]` (Java
//! `ValidateCommand`, spec `function-manifest-authoring.md` M2.3 in the Java
//! repo): `POST /api/functions/{address}/manifest/check`, then the errors,
//! or the plan as a readable list. `--output json` prints the response body
//! verbatim. Never publishes or promotes anything.
//!
//! Exit 0 when `valid`, 1 when not. `settingsMissing` alone (a promote
//! precondition, not a publish one) never fails the command: it is printed
//! as a warning line, as are the platform's `warnings` (a pool with no live
//! host, or one whose runtimes are unknown).

use std::io::Write;
use std::path::PathBuf;

use serde_json::{json, Value};

use super::deploy::read_manifest;
use super::{print_json, AddressOpts, CliError, Ctx, Io, OutputMode};

#[derive(clap::Args, Debug)]
pub struct ValidateArgs {
    /// Full function address, app.service.name.
    pub address: Option<String>,

    /// The manifest JSON file.
    #[arg(long, value_name = "FILE")]
    pub manifest: PathBuf,

    /// The alias to preview promoting to.
    #[arg(long, value_name = "NAME", default_value = "live")]
    pub alias: String,

    #[command(flatten)]
    pub address_opts: AddressOpts,
}

pub async fn run(ctx: &Ctx<'_>, args: &ValidateArgs, io: &mut Io<'_>) -> Result<i32, CliError> {
    let address = args.address_opts.resolve(args.address.as_deref())?;
    let manifest = read_manifest(&args.manifest)?;
    let client = ctx.client()?;
    let response = client
        .post(
            &format!("/api/functions/{address}/manifest/check"),
            json!({"manifest": manifest, "alias": args.alias}),
        )
        .await?
        .unwrap_or(Value::Null);
    let valid = response
        .get("valid")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    match ctx.output() {
        OutputMode::Json => print_json(io.out, &response)?,
        OutputMode::Text => print_text(io.out, &response, valid)?,
    }
    Ok(if valid { 0 } else { 1 })
}

fn text<'a>(node: &'a Value, key: &str) -> &'a str {
    node.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn items<'a>(node: &'a Value, key: &str) -> &'a [Value] {
    node.get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn changed_fields(action: &Value) -> String {
    items(action, "changedFields")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Every problem, one per line (`code pointer: message`); or the plan, one
/// line per wiring change, then conflicts and warnings (Java's text form).
pub fn print_text(out: &mut dyn Write, response: &Value, valid: bool) -> std::io::Result<()> {
    if !valid {
        for error in items(response, "errors") {
            let pointer = error
                .pointer("/details/pointer")
                .and_then(Value::as_str)
                .unwrap_or_default();
            writeln!(
                out,
                "{} {}: {}",
                text(error, "code"),
                pointer,
                text(error, "message")
            )?;
        }
        return Ok(());
    }

    let plan = response.get("plan").cloned().unwrap_or(Value::Null);
    let http_only = plan
        .get("httpOnly")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut wiring = false;
    if http_only {
        writeln!(out, "! named alias — no wiring change")?;
    } else {
        if let Some(pool) = plan.get("pool") {
            match text(pool, "action") {
                "create" => {
                    writeln!(out, "+ pool (create)")?;
                    wiring = true;
                }
                "update" => {
                    writeln!(out, "~ pool (update: {})", changed_fields(pool))?;
                    wiring = true;
                }
                _ => {}
            }
        }
        for a in items(&plan, "subscriptions") {
            let event_type = text(a, "eventType");
            match text(a, "action") {
                "create" => writeln!(out, "+ subscription {event_type} (create)")?,
                "update" => writeln!(
                    out,
                    "~ subscription {event_type} (update: {})",
                    changed_fields(a)
                )?,
                "delete" => writeln!(out, "- subscription {event_type} (delete)")?,
                _ => continue,
            }
            wiring = true;
        }
        for a in items(&plan, "schedules") {
            let cron = text(a, "cron");
            match text(a, "action") {
                "create" => writeln!(out, "+ schedule \"{cron}\" (create)")?,
                "update" => writeln!(out, "~ schedule \"{cron}\" (update: {})", changed_fields(a))?,
                "delete" => writeln!(out, "- schedule \"{cron}\" (delete)")?,
                _ => continue,
            }
            wiring = true;
        }
        if let Some(routes) = plan
            .get("publicRoutes")
            .filter(|r| text(r, "action") == "replace")
        {
            for (sign, key) in [("+", "added"), ("-", "removed")] {
                for r in items(routes, key) {
                    writeln!(
                        out,
                        "{sign} route {}{}",
                        text(r, "hostname"),
                        text(r, "pathPrefix")
                    )?;
                    wiring = true;
                }
            }
        }
    }

    let mut flagged = false;
    for c in items(&plan, "conflicts") {
        writeln!(
            out,
            "! conflict: {}: {}",
            text(c, "code"),
            text(c, "message")
        )?;
        flagged = true;
    }
    for w in items(&plan, "warnings") {
        writeln!(
            out,
            "! warning: {}: {}",
            text(w, "code"),
            text(w, "message")
        )?;
        flagged = true;
    }
    let missing: Vec<&str> = items(&plan, "settingsMissing")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    if !missing.is_empty() {
        writeln!(out, "! settings missing: {}", missing.join(", "))?;
        flagged = true;
    }
    if !http_only && !wiring && !flagged {
        writeln!(out, "no changes")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn printed(response: Value) -> String {
        let valid = response["valid"].as_bool().unwrap();
        let mut out = Vec::new();
        print_text(&mut out, &response, valid).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn errors_print_one_per_line_with_their_pointer() {
        let response = json!({"valid": false, "errors": [
            {"code": "RUNTIME_INVALID", "message": "runtime must be …", "details": {"pointer": "/runtime"}},
            {"code": "APPLICATION_SIGNING_SECRET_REQUIRED", "message": "no secret", "details": {}}
        ]});
        assert_eq!(
            printed(response),
            "RUNTIME_INVALID /runtime: runtime must be …\n\
             APPLICATION_SIGNING_SECRET_REQUIRED : no secret\n"
        );
    }

    /// Java's text form; `settingsMissing` and warnings only warn.
    #[test]
    fn the_plan_prints_one_line_per_change_then_what_needs_attention() {
        let response = json!({"valid": true, "errors": [], "plan": {
            "alias": "live", "toVersion": 2, "httpOnly": false,
            "settingsMissing": ["CARRIER_API_KEY"],
            "pool": {"action": "update", "key": "k", "changedFields": ["concurrency"]},
            "subscriptions": [
                {"action": "create", "eventType": "shop:orders:order:placed", "changedFields": []},
                {"action": "unchanged", "eventType": "shop:orders:order:kept", "changedFields": []},
                {"action": "delete", "eventType": "shop:orders:order:gone", "changedFields": []}
            ],
            "schedules": [{"action": "update", "cron": "0 0 * * * *", "changedFields": ["timezone"]}],
            "publicRoutes": {"action": "replace",
                "added": [{"hostname": "api.shop.test", "pathPrefix": "/v2", "aliasPrefixes": []}],
                "removed": [{"hostname": "api.shop.test", "pathPrefix": "/v1", "aliasPrefixes": []}]},
            "conflicts": [{"code": "PUBLIC_ROUTE_TAKEN", "message": "taken"}],
            "warnings": [{"code": "POOL_HAS_NO_LIVE_HOSTS", "message": "no host"}]
        }});
        assert_eq!(
            printed(response),
            "~ pool (update: concurrency)\n\
             + subscription shop:orders:order:placed (create)\n\
             - subscription shop:orders:order:gone (delete)\n\
             ~ schedule \"0 0 * * * *\" (update: timezone)\n\
             + route api.shop.test/v2\n\
             - route api.shop.test/v1\n\
             ! conflict: PUBLIC_ROUTE_TAKEN: taken\n\
             ! warning: POOL_HAS_NO_LIVE_HOSTS: no host\n\
             ! settings missing: CARRIER_API_KEY\n"
        );
    }

    #[test]
    fn nothing_to_change_says_so_and_a_named_alias_has_no_wiring() {
        let unchanged = json!({"valid": true, "errors": [], "plan": {
            "httpOnly": false, "settingsMissing": [],
            "pool": {"action": "unchanged", "changedFields": []},
            "subscriptions": [], "schedules": [],
            "publicRoutes": {"action": "unchanged", "added": [], "removed": []},
            "conflicts": [], "warnings": []
        }});
        assert_eq!(printed(unchanged), "no changes\n");
        let named = json!({"valid": true, "errors": [], "plan": {
            "httpOnly": true, "settingsMissing": [], "subscriptions": [], "schedules": [],
            "conflicts": [], "warnings": []
        }});
        assert_eq!(printed(named), "! named alias — no wiring change\n");
    }

    #[test]
    fn the_manifest_is_required_and_the_alias_defaults_to_live() {
        let args = super::super::parse(&["validate", "a.b.c", "--manifest", "m.json"]).unwrap();
        match args.command {
            super::super::FnCommand::Validate(v) => {
                assert_eq!(v.alias, "live");
                assert_eq!(v.address.as_deref(), Some("a.b.c"));
            }
            other => panic!("{other:?}"),
        }
        assert!(super::super::parse(&["validate", "a.b.c"]).is_err());
    }
}
