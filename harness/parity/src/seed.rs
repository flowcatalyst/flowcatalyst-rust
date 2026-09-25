//! Builds `seed` (spec §1) and clones it: Go's `fcdev init` (goose
//! migrations + system seed + anchor admin + default client + application,
//! plus the service account and OAuth client Go's init always mints), then a
//! Go `fc-server` start-and-stop against it to prove the day-one boot path,
//! then `CREATE DATABASE parity_go / parity_rust TEMPLATE seed`. `seed` holds
//! exactly what a Go deployment holds on day one; Rust's `fc-server` then
//! boots on its clone, which is the real Go-to-Rust database handover
//! (Rust's migration runner adopts the goose schema, its startup seeders run).
//!
//! ## The Go-seeder workaround, without Java
//!
//! Go `cb83fd5` could not bootstrap a fresh database: its seeder wrote
//! `schema_type = 'JSON'`, which migration 051's own CHECK
//! (`chk_msg_event_type_spec_versions_schema_type`) rejects, so `fcdev init`
//! aborted after migrations. Java's harness filled the catalogue with the
//! Java seeder and re-ran `fcdev init`. This port needs no Java: on exactly
//! that failure it installs a `BEFORE INSERT` trigger on
//! `msg_event_type_spec_versions` that rewrites `'JSON'` to `'JSON_SCHEMA'`
//! (the value Go's own later seeder writes), re-runs `fcdev init` (idempotent:
//! it skips every event type and spec version already present), and drops the
//! trigger again. Every row stays Go-written. Go HEAD at the time of the port
//! (`73a6918`) writes `JSON_SCHEMA` itself, so the first init succeeds and
//! the workaround is never entered; the run log says which path was taken.

use anyhow::{bail, Context, Result};
use indexmap::IndexMap;
use sqlx::{Connection, PgConnection};
use std::path::Path;
use std::process::Command;

use crate::binaries::GoBinaries;
use crate::pg::DockerPg;
use crate::side::SubprocessSide;
use crate::vars::SeedIds;

pub const KNOWN_DEFECT_CONSTRAINT: &str = "chk_msg_event_type_spec_versions_schema_type";
pub const ADMIN_EMAIL: &str = "parity-admin@example.com";
/// No identity words (`PASSWORD_CONTAINS_IDENTITY`).
pub const ADMIN_PASSWORD: &str = "Correct-Harness-Battery-9147";
pub const APP_CODE: &str = "parity";
pub const APP_NAME: &str = "Parity";
pub const CLIENT_IDENTIFIER: &str = "default";

pub const GO_DB: &str = "parity_go";
pub const RUST_DB: &str = "parity_rust";

pub struct SeedResult {
    pub ids: SeedIds,
    /// Whether the known Go seeder defect was hit and the trigger path taken.
    pub worked_around: bool,
}

const WORKAROUND_INSTALL: &str = r#"
CREATE OR REPLACE FUNCTION parity_fix_schema_type() RETURNS trigger AS $$
BEGIN
    IF NEW.schema_type = 'JSON' THEN
        NEW.schema_type := 'JSON_SCHEMA';
    END IF;
    RETURN NEW;
END
$$ LANGUAGE plpgsql;
CREATE TRIGGER parity_fix_schema_type
    BEFORE INSERT ON msg_event_type_spec_versions
    FOR EACH ROW EXECUTE FUNCTION parity_fix_schema_type();
"#;

const WORKAROUND_REMOVE: &str = r#"
DROP TRIGGER IF EXISTS parity_fix_schema_type ON msg_event_type_spec_versions;
DROP FUNCTION IF EXISTS parity_fix_schema_type();
"#;

pub async fn build(
    go: &GoBinaries,
    pg: &DockerPg,
    scratch: &Path,
    base_env: &IndexMap<String, String>,
    log_dir: &Path,
) -> Result<SeedResult> {
    pg.create_database("seed").await?;
    let seed_url = pg.url("seed");

    let first = fcdev_init(go, &seed_url, scratch, base_env)?;
    let worked_around = if first.0 {
        tracing::info!("fcdev init succeeded on the first attempt: Go bootstraps a fresh database");
        false
    } else if first.1.contains(KNOWN_DEFECT_CONSTRAINT) {
        tracing::warn!(
            "fcdev init hit the known Go seeder defect ({KNOWN_DEFECT_CONSTRAINT}); \
             re-running it with the schema_type trigger installed"
        );
        pg.execute("seed", WORKAROUND_INSTALL).await?;
        let second = fcdev_init(go, &seed_url, scratch, base_env)?;
        pg.execute("seed", WORKAROUND_REMOVE).await?;
        if !second.0 {
            bail!(
                "fcdev init failed even with the seeder workaround:\n{}",
                second.1
            );
        }
        true
    } else {
        bail!("fcdev init failed:\n{}", first.1);
    };

    let mut env = base_env.clone();
    env.insert("FC_DATABASE_URL".into(), seed_url.clone());
    let mut seed_server =
        SubprocessSide::start("go-seed", &go.fc_server, &env, &log_dir.join("go-seed.log")).await?;
    seed_server.stop().await;

    let ids = read_ids(&seed_url).await?;
    tracing::info!(client = %ids.client_id, app = %ids.app_id, admin = %ids.admin_id, "seed ids");

    pg.create_from_template(GO_DB, "seed").await?;
    pg.create_from_template(RUST_DB, "seed").await?;
    Ok(SeedResult { ids, worked_around })
}

/// `(succeeded, combined output)`.
fn fcdev_init(
    go: &GoBinaries,
    url: &str,
    root: &Path,
    base_env: &IndexMap<String, String>,
) -> Result<(bool, String)> {
    let out = Command::new(&go.fcdev)
        .args(["init", "--yes", "--database-url", url])
        .args([
            "--admin-email",
            ADMIN_EMAIL,
            "--admin-password",
            ADMIN_PASSWORD,
        ])
        .args([
            "--code",
            APP_CODE,
            "--name",
            APP_NAME,
            "--client-identifier",
            CLIENT_IDENTIFIER,
        ])
        .arg("--root")
        .arg(root)
        .env(
            "FLOWCATALYST_APP_KEY",
            base_env
                .get("FLOWCATALYST_APP_KEY")
                .cloned()
                .unwrap_or_default(),
        )
        .output()
        .context("run fcdev init")?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    tracing::debug!(output = %text, "fcdev init");
    Ok((out.status.success(), text))
}

async fn read_ids(url: &str) -> Result<SeedIds> {
    let mut c = PgConnection::connect(url).await?;
    let app_id: Option<(String,)> =
        sqlx::query_as("SELECT id FROM app_applications WHERE code = $1")
            .bind(APP_CODE)
            .fetch_optional(&mut c)
            .await?;
    let client_id: Option<(String,)> =
        sqlx::query_as("SELECT id FROM tnt_clients WHERE identifier = $1")
            .bind(CLIENT_IDENTIFIER)
            .fetch_optional(&mut c)
            .await?;
    let admin_id: Option<(String,)> =
        sqlx::query_as("SELECT id FROM iam_principals WHERE email = $1")
            .bind(ADMIN_EMAIL.to_lowercase())
            .fetch_optional(&mut c)
            .await?;
    c.close().await?;
    Ok(SeedIds {
        app_id: app_id
            .with_context(|| format!("seed has no application coded '{APP_CODE}'"))?
            .0,
        client_id: client_id
            .with_context(|| format!("seed has no client identified '{CLIENT_IDENTIFIER}'"))?
            .0,
        admin_id: admin_id
            .with_context(|| format!("seed has no principal with email {ADMIN_EMAIL}"))?
            .0,
    })
}
