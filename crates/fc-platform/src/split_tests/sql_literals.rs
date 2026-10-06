//! The statements that spell an enum as a SQL literal on purpose, tied to the
//! enum's spelling.
//!
//! Most statements bind the enum (`status = $1`), so a renamed variant moves
//! the SQL with it. A few cannot: a statement must match a partial index's
//! predicate (`WHERE status = 'PENDING'`) or sit on the dispatch hot path, where
//! measurement showed the planner needs the literal and the
//! `status || '' = 'PENDING'` forms, so they are written out. Each is listed
//! here with the enum it names, and the test fails if the statement's text and
//! the enum's spelling part company: rename a variant and this tells you which
//! statements to edit, and the partial indexes to migrate with them.

use std::fs;

use crate::application_openapi_spec::entity::OpenApiSpecStatus;
use crate::dispatch_job::entity::{
    parse_dispatch_mode, parse_dispatch_status, DispatchMode, DispatchStatus,
};
use crate::platform_config::entity::ConfigValueType;
use crate::scheduled_job::entity::{InstanceStatus, ScheduledJobStatus};
use crate::shared::secret_backfill::SECRET_COLUMNS;
use crate::subscription::entity::SubscriptionStatus;

/// A source file of this workspace, by its path from `crates/`.
fn source(path: &str) -> String {
    let full = format!("{}/../{path}", env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(&full).unwrap_or_else(|e| panic!("{full}: {e}"))
}

/// `file` spells each of `literals` quoted, as the enum does.
fn spells(file: &str, literals: &[&str]) {
    let text = source(file);
    for literal in literals {
        assert!(
            text.contains(&format!("'{literal}'")),
            "{file} no longer holds the literal '{literal}' its enum spells"
        );
    }
}

#[test]
fn dispatch_lifecycle_statements_spell_the_enum() {
    spells(
        "fc-common/src/dispatch_lifecycle.rs",
        &[
            DispatchStatus::Pending.as_str(),
            DispatchStatus::Queued.as_str(),
            DispatchStatus::Processing.as_str(),
            DispatchStatus::Failed.as_str(),
            DispatchMode::BlockOnError.as_str(),
        ],
    );
    // The legacy spelling of FAILED those statements also match (`status IN
    // ('FAILED', 'ERROR')`, the blocked-groups index) is still read as FAILED.
    spells("fc-common/src/dispatch_lifecycle.rs", &["ERROR"]);
    assert_eq!(parse_dispatch_status("ERROR"), Ok(DispatchStatus::Failed));
    assert_eq!(
        parse_dispatch_mode(Some(DispatchMode::BlockOnError.as_str())),
        DispatchMode::BlockOnError
    );
}

#[test]
fn dispatch_job_reaper_and_poller_statements_spell_the_enum() {
    let file = "fc-platform-messaging/src/dispatch_job/repository.rs";
    let text = source(file);
    for status in [DispatchStatus::Pending, DispatchStatus::Processing] {
        assert!(
            text.contains(&format!("status = '{}'", status.as_str())),
            "{file} no longer holds status = '{}'",
            status.as_str()
        );
    }
}

#[test]
fn the_dispatch_job_projection_spells_the_terminal_statuses() {
    spells(
        "fc-stream/src/dispatch_job_projection.rs",
        &[
            DispatchStatus::Completed.as_str(),
            DispatchStatus::Failed.as_str(),
            DispatchStatus::Cancelled.as_str(),
            DispatchStatus::Expired.as_str(),
        ],
    );
}

#[test]
fn the_event_fan_out_spells_the_active_subscription_status() {
    // fc-stream cannot name `SubscriptionStatus` (the messaging crate depends
    // on it, not the other way round).
    let file = "fc-stream/src/event_fan_out.rs";
    let text = source(file);
    let want = format!("s.status = '{}'", SubscriptionStatus::Active.as_str());
    assert!(text.contains(&want), "{file} no longer holds {want}");
}

#[test]
fn scheduled_job_statements_spell_the_enum() {
    // `idx_msg_scheduled_jobs_active_poll` and the instance claim, mark and
    // active-count statements match partial-index predicates.
    let jobs = source("fc-platform-scheduled-jobs/src/scheduled_job/repository.rs");
    let want = format!("status = '{}'", ScheduledJobStatus::Active.as_str());
    assert!(jobs.contains(&want), "the job poll no longer holds {want}");
    spells(
        "fc-platform-scheduled-jobs/src/scheduled_job/instance_repository.rs",
        &[
            InstanceStatus::Queued.as_str(),
            InstanceStatus::InFlight.as_str(),
            InstanceStatus::Delivered.as_str(),
        ],
    );
}

#[test]
fn the_current_openapi_spec_lookups_spell_the_enum() {
    // The partial unique index is `WHERE status = 'CURRENT'`.
    let file = "fc-platform-iam/src/application_openapi_spec/repository.rs";
    let want = format!("status = '{}'", OpenApiSpecStatus::Current.as_str());
    assert!(
        source(file).contains(&want),
        "{file} no longer holds {want}"
    );
}

#[test]
fn the_secret_backfill_filter_spells_the_secret_value_type() {
    let filter = SECRET_COLUMNS
        .iter()
        .find(|c| c.table == "app_platform_configs")
        .and_then(|c| c.row_filter)
        .expect("the platform-config secret column has a row filter");
    assert_eq!(
        filter,
        format!("value_type = '{}'", ConfigValueType::Secret.as_str())
    );
}
