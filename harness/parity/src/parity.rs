//! The whole pipeline (spec §1–§7), shared by the CLI and the `#[ignore]`
//! test: build, seed, start both sides, run every scenario against each
//! (Go first, then Rust), normalise + diff + coverage, write the report,
//! stop both sides, remove the database container.

use anyhow::{Context, Result};
use indexmap::IndexMap;
use std::path::{Path, PathBuf};

use crate::binaries::{self, GoBinaries, RustBinaries};
use crate::coverage::{self, RequestedRoute};
use crate::diff::{self, DiffEntry};
use crate::expected::{ExpectedDiffs, GO_EXPECT};
use crate::keys;
use crate::loader;
use crate::model::{Scenario, Step};
use crate::normaliser;
use crate::pg::DockerPg;
use crate::report::{Report, ScenarioResult, StepResult, StepStatus};
use crate::runner::{Runner, StepOutcome};
use crate::seed::{self, ADMIN_EMAIL, ADMIN_PASSWORD};
use crate::side::SubprocessSide;
use crate::vars::{SeedIds, Vars};

#[derive(Debug, Clone)]
pub struct Config {
    /// Go source tree to build `fcdev` / `fc-server` from.
    pub go_src: PathBuf,
    /// Prebuilt Go `fcdev` + `fc-server` (skips the Go build).
    pub go_bin_dir: Option<PathBuf>,
    /// Where a Go build writes its binaries (outside the Go tree).
    pub go_out_dir: PathBuf,
    /// Prebuilt Rust `fc-server` (skips the cargo build).
    pub rust_bin_dir: Option<PathBuf>,
    /// This workspace (for the cargo build and the Rust commit label).
    pub workspace: PathBuf,
    pub scenarios_dir: PathBuf,
    pub only: Option<String>,
    pub report_dir: PathBuf,
    pub surface_file: PathBuf,
    pub lockfile_file: PathBuf,
    pub expected_diffs_file: PathBuf,
    pub pg_image: String,
}

impl Config {
    /// Defaults relative to this crate: vendored scenarios / surface /
    /// lockfile / allow-list, `../flowcatalyst-go` beside the workspace,
    /// `target/go-bin` and `target/parity-report` inside it.
    pub fn defaults() -> Self {
        let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace = crate_dir
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| crate_dir.clone());
        Self {
            go_src: workspace.join("../flowcatalyst-go"),
            go_bin_dir: None,
            go_out_dir: workspace.join("target/go-bin"),
            rust_bin_dir: None,
            scenarios_dir: crate_dir.join("scenarios"),
            only: None,
            report_dir: workspace.join("target/parity-report"),
            surface_file: crate_dir.join("surface.json"),
            lockfile_file: crate_dir.join("lockfile-operations.json"),
            expected_diffs_file: crate_dir.join("expected-diffs.json"),
            pg_image: "postgres:18".into(),
            workspace,
        }
    }
}

pub async fn run(config: &Config) -> Result<Report> {
    std::fs::create_dir_all(&config.report_dir)
        .with_context(|| format!("create {}", config.report_dir.display()))?;
    let scratch = std::env::temp_dir().join(format!("fc-parity-{}", keys::random_token()));
    std::fs::create_dir_all(&scratch)?;
    let result = run_in(config, &scratch).await;
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

async fn run_in(config: &Config, scratch: &Path) -> Result<Report> {
    // Fail on a bad scenario file or allow-list before any build or container.
    let scenarios = loader::load(&config.scenarios_dir, config.only.as_deref())?;
    let expected = ExpectedDiffs::load(&config.expected_diffs_file)?;
    let lockfile = coverage::load_lockfile(&config.lockfile_file)?;
    let surface = coverage::load_surface(&config.surface_file)?;

    let go = binaries::resolve_go(
        &config.go_src,
        config.go_bin_dir.as_deref(),
        &config.go_out_dir,
    )?;
    let rust = binaries::resolve_rust(&config.workspace, config.rust_bin_dir.as_deref())?;

    let private_key = scratch.join("jwt-signing-key.pem");
    let public_key = scratch.join("jwt-signing-key.pub.pem");
    keys::generate_rsa_pems(&private_key, &public_key)?;
    let env = base_env(&private_key, &public_key, &keys::generate_app_key());

    let pg = DockerPg::start(&config.pg_image).await?;
    let seed = seed::build(&go, &pg, scratch, &env, &config.report_dir).await?;
    let seed_path = if seed.worked_around {
        "Go fcdev init, second attempt with the schema_type trigger (known Go seeder defect)"
    } else {
        "Go fcdev init, first attempt"
    };

    let mut go_side = start_side("go", &go.fc_server, &env, &pg.url(seed::GO_DB), config).await?;
    let mut rust_side = match start_side(
        "rust",
        &rust.fc_server,
        &env,
        &pg.url(seed::RUST_DB),
        config,
    )
    .await
    {
        Ok(side) => side,
        Err(e) => {
            go_side.stop().await;
            return Err(e);
        }
    };

    let report = run_scenarios(
        &scenarios,
        &seed.ids,
        &go_side,
        &rust_side,
        &expected,
        &lockfile.operations,
        &surface,
        config,
        &go,
        &rust,
        seed_path,
    )
    .await;
    rust_side.stop().await;
    go_side.stop().await;
    tracing::info!(
        go_start = ?go_side.start_duration,
        rust_start = ?rust_side.start_duration,
        container = pg.container(),
        "sides stopped; removing the database container"
    );
    drop(pg);
    let report = report?;
    report.write_json(&config.report_dir.join("report.json"))?;
    report.write_markdown(&config.report_dir.join("report.md"))?;
    Ok(report)
}

async fn start_side(
    label: &str,
    binary: &Path,
    env: &IndexMap<String, String>,
    database_url: &str,
    config: &Config,
) -> Result<SubprocessSide> {
    let mut env = env.clone();
    env.insert("FC_DATABASE_URL".into(), database_url.to_string());
    SubprocessSide::start(
        label,
        binary,
        &env,
        &config.report_dir.join(format!("{label}.log")),
    )
    .await
}

/// The one environment map handed to both sides (spec §2); a knob one side
/// does not know is harmless. Per-side values (`FC_DATABASE_URL`, port,
/// issuer, origins) are added by [`SubprocessSide::start`].
pub fn base_env(private_key: &Path, public_key: &Path, app_key: &str) -> IndexMap<String, String> {
    let mut env = IndexMap::new();
    let mut set = |k: &str, v: &str| {
        env.insert(k.to_string(), v.to_string());
    };
    set("FC_PLATFORM_ENABLED", "true");
    for off in [
        "FC_ROUTER_ENABLED",
        "FC_SCHEDULER_ENABLED",
        "FC_SCHEDULED_JOB_ENABLED",
        "FC_STREAM_PROCESSOR_ENABLED",
        "FC_OUTBOX_ENABLED",
        "FC_MCP_ENABLED",
        "FC_STANDBY_ENABLED",
        "FC_ALB_ENABLED",
    ] {
        set(off, "false");
    }
    // Go reads one PKCS#8 file; Rust reads the pair. Same key either way.
    set("FC_JWT_SIGNING_KEY_PATH", &private_key.to_string_lossy());
    set("FC_JWT_PRIVATE_KEY_PATH", &private_key.to_string_lossy());
    set("FC_JWT_PUBLIC_KEY_PATH", &public_key.to_string_lossy());
    set("FLOWCATALYST_APP_KEY", app_key);
    set("FC_WEBAUTHN_RP_ID", "localhost");
    // The anchor already exists (fcdev init made it); set for completeness.
    set("FC_BOOTSTRAP_ADMIN_EMAIL", ADMIN_EMAIL);
    set("FC_BOOTSTRAP_ADMIN_PASSWORD", ADMIN_PASSWORD);
    set("FLOWCATALYST_BOOTSTRAP_ADMIN_EMAIL", ADMIN_EMAIL);
    set("FLOWCATALYST_BOOTSTRAP_ADMIN_PASSWORD", ADMIN_PASSWORD);
    if std::env::var_os("RUST_LOG").is_none() {
        set("RUST_LOG", "info");
    }
    env
}

#[allow(clippy::too_many_arguments)]
async fn run_scenarios(
    scenarios: &[loader::Loaded],
    ids: &SeedIds,
    go_side: &SubprocessSide,
    rust_side: &SubprocessSide,
    expected: &ExpectedDiffs,
    lockfile: &[coverage::Route],
    surface: &[coverage::Route],
    config: &Config,
    go: &GoBinaries,
    rust: &RustBinaries,
    seed_path: &str,
) -> Result<Report> {
    let run = keys::random_token();
    let mut results = Vec::new();
    let mut all_requested: Vec<RequestedRoute> = Vec::new();
    let mut go_labels = IndexMap::new();
    let mut rust_labels = IndexMap::new();

    for loaded in scenarios {
        let scenario = &loaded.scenario;
        let mut go_vars = Vars::new(
            ADMIN_EMAIL,
            ADMIN_PASSWORD,
            &run,
            ids.clone(),
            std::mem::take(&mut go_labels),
        );
        let mut rust_vars = Vars::new(
            ADMIN_EMAIL,
            ADMIN_PASSWORD,
            &run,
            ids.clone(),
            std::mem::take(&mut rust_labels),
        );
        let go_run = Runner::new(&go_side.base_url)?
            .run(scenario, &mut go_vars)
            .await;
        let rust_run = Runner::new(&rust_side.base_url)?
            .run(scenario, &mut rust_vars)
            .await;
        all_requested.extend(go_run.requested.iter().cloned());
        all_requested.extend(rust_run.requested.iter().cloned());

        let steps: Vec<StepResult> = scenario
            .steps
            .iter()
            .zip(go_run.steps.iter().zip(&rust_run.steps))
            .map(|(step, (g, r))| {
                compare_step(
                    scenario,
                    step,
                    g,
                    r,
                    (&go_vars, &go_side.base_url),
                    (&rust_vars, &rust_side.base_url),
                    expected,
                )
            })
            .collect();

        let mut unmet: Vec<String> = Vec::new();
        for claim in coverage::unmet_claims(&scenario.covers, lockfile, &go_run.requested)
            .into_iter()
            .chain(coverage::unmet_claims(
                &scenario.covers,
                lockfile,
                &rust_run.requested,
            ))
        {
            if !unmet.contains(&claim) {
                unmet.push(claim);
            }
        }
        let result = ScenarioResult {
            file: loaded.relative_path.clone(),
            scenario_name: scenario.name.clone(),
            steps,
            unmet_covers_claims: unmet,
        };
        tracing::info!(
            file = %result.file,
            ok = result.count(StepStatus::Ok),
            accepted = result.count(StepStatus::Accepted),
            diff = result.count(StepStatus::Diff),
            error = result.count(StepStatus::Error),
            "scenario done"
        );
        results.push(result);
        go_labels = go_vars.into_run_labels();
        rust_labels = rust_vars.into_run_labels();
    }

    let coverage = coverage::compute(lockfile, surface, &all_requested);
    // Stale entries only mean something on a full run.
    let full_run = config.only.as_deref().is_none_or(|g| g.trim().is_empty());
    let stale_entries = if full_run {
        expected.stale()
    } else {
        Vec::new()
    };
    Ok(Report {
        go_label: go.label.clone(),
        rust_label: rust.label.clone(),
        seed_path: seed_path.to_string(),
        scenarios: results,
        coverage,
        stale_entries,
    })
}

/// Compares one step's two outcomes, after both sides ran the whole
/// scenario (so rule 1 sees every capture of the scenario).
pub fn compare_step(
    scenario: &Scenario,
    step: &Step,
    go: &StepOutcome,
    rust: &StepOutcome,
    (go_vars, go_base): (&Vars, &str),
    (rust_vars, rust_base): (&Vars, &str),
    expected: &ExpectedDiffs,
) -> StepResult {
    if let Some(known) = known_go_failure(&scenario.name, step, go, rust, expected) {
        return known;
    }
    let (go_rec, rust_rec) = match (go, rust) {
        (StepOutcome::Ran(g), StepOutcome::Ran(r)) => (g, r),
        _ => {
            let message = |o: &StepOutcome| match o {
                StepOutcome::Failed { message, .. } => Some(message.clone()),
                StepOutcome::Ran(_) => None,
            };
            let error = match (message(go), message(rust)) {
                (Some(g), Some(r)) => format!("go: {g} | rust: {r}"),
                (Some(g), None) => format!("go: {g}"),
                (None, Some(r)) => format!("rust: {r}"),
                (None, None) => unreachable!("at least one side failed"),
            };
            return StepResult {
                step_id: step.id.clone(),
                status: StepStatus::Error,
                diffs: Vec::new(),
                unaccepted: Vec::new(),
                go_record: record_of(go),
                rust_record: record_of(rust),
                error: Some(error),
            };
        }
    };
    let g = normaliser::normalise(go_rec, go_vars, go_base, step);
    let r = normaliser::normalise(rust_rec, rust_vars, rust_base, step);
    let diffs = diff::compare(&g, &r);
    let unaccepted: Vec<DiffEntry> = diffs
        .iter()
        .filter(|d| !expected.accepts(&scenario.name, &step.id, &d.pointer))
        .cloned()
        .collect();
    let status = if diffs.is_empty() {
        StepStatus::Ok
    } else if unaccepted.is_empty() {
        StepStatus::Accepted
    } else {
        StepStatus::Diff
    };
    let keep = status != StepStatus::Ok;
    StepResult {
        step_id: step.id.clone(),
        status,
        diffs,
        unaccepted,
        go_record: keep.then(|| go_rec.clone()),
        rust_record: keep.then(|| rust_rec.clone()),
        error: None,
    }
}

/// A step Go is known to fail (`!go-expect`): Go answered but missed the
/// step's `expect.status`, Rust met it, and an allow-list entry names this
/// step. Then the step is `ACCEPTED` and Go's response is not compared.
fn known_go_failure(
    scenario: &str,
    step: &Step,
    go: &StepOutcome,
    rust: &StepOutcome,
    expected: &ExpectedDiffs,
) -> Option<StepResult> {
    let (
        StepOutcome::Failed {
            message,
            record: Some(go_record),
        },
        StepOutcome::Ran(rust_record),
    ) = (go, rust)
    else {
        return None;
    };
    if !expected.accepts(scenario, &step.id, GO_EXPECT) {
        return None;
    }
    Some(StepResult {
        step_id: step.id.clone(),
        status: StepStatus::Accepted,
        diffs: vec![DiffEntry::new(
            GO_EXPECT,
            message.clone(),
            "expected status met",
        )],
        unaccepted: Vec::new(),
        go_record: Some(go_record.clone()),
        rust_record: Some(rust_record.clone()),
        error: None,
    })
}

fn record_of(o: &StepOutcome) -> Option<crate::record::StepRecord> {
    match o {
        StepOutcome::Ran(r) => Some(r.clone()),
        StepOutcome::Failed { record, .. } => record.clone(),
    }
}
