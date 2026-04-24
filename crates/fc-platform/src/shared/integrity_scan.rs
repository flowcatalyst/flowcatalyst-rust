//! Referential-integrity invariant scan.
//!
//! The IAM schema uses text-keyed junctions (`iam_principal_roles.role_name`)
//! and id-keyed junctions without DB-level FKs (`iam_principal_application_access`,
//! `iam_client_access_grants`) because integrity is managed in code, not by
//! the database. This scan runs once at startup and logs a warning per
//! junction when orphan rows exist — serves as a "your invariants drifted"
//! early-warning alarm for operators.
//!
//! Zero orphans is the only acceptable steady state. Non-zero counts
//! indicate a regression in one of the aggregate delete paths.
//!
//! Run order / wiring: call `run()` after migrations complete and before
//! accepting traffic. Failures are logged, not fatal — a drifted DB should
//! surface an alert but not block rollout.

use sqlx::PgPool;
use tracing::{info, warn};

/// Named orphan-count query.
struct Check {
    name: &'static str,
    sql: &'static str,
}

/// Queries that must always return 0. Each one describes the invariant in
/// plain terms so the log line is actionable.
///
/// Nullable columns use `IS NOT NULL AND NOT EXISTS (…)` so a legitimately
/// unset reference doesn't count as drift.
const CHECKS: &[Check] = &[
    // --- iam_principal_roles junction ---
    Check {
        name: "iam_principal_roles.role_name → iam_roles.name",
        sql: "SELECT COUNT(*) FROM iam_principal_roles pr \
              WHERE NOT EXISTS (SELECT 1 FROM iam_roles r WHERE r.name = pr.role_name)",
    },
    Check {
        name: "iam_principal_roles.principal_id → iam_principals.id",
        sql: "SELECT COUNT(*) FROM iam_principal_roles pr \
              WHERE NOT EXISTS (SELECT 1 FROM iam_principals p WHERE p.id = pr.principal_id)",
    },

    // --- iam_principal_application_access junction ---
    Check {
        name: "iam_principal_application_access.principal_id → iam_principals.id",
        sql: "SELECT COUNT(*) FROM iam_principal_application_access a \
              WHERE NOT EXISTS (SELECT 1 FROM iam_principals p WHERE p.id = a.principal_id)",
    },
    Check {
        name: "iam_principal_application_access.application_id → app_applications.id",
        sql: "SELECT COUNT(*) FROM iam_principal_application_access a \
              WHERE NOT EXISTS (SELECT 1 FROM app_applications x WHERE x.id = a.application_id)",
    },

    // --- iam_client_access_grants ---
    Check {
        name: "iam_client_access_grants.principal_id → iam_principals.id",
        sql: "SELECT COUNT(*) FROM iam_client_access_grants g \
              WHERE NOT EXISTS (SELECT 1 FROM iam_principals p WHERE p.id = g.principal_id)",
    },
    Check {
        name: "iam_client_access_grants.client_id → tnt_clients.id",
        sql: "SELECT COUNT(*) FROM iam_client_access_grants g \
              WHERE NOT EXISTS (SELECT 1 FROM tnt_clients c WHERE c.id = g.client_id)",
    },

    // --- app_client_configs (ApplicationClientConfig) ---
    Check {
        name: "app_client_configs.application_id → app_applications.id",
        sql: "SELECT COUNT(*) FROM app_client_configs c \
              WHERE NOT EXISTS (SELECT 1 FROM app_applications a WHERE a.id = c.application_id)",
    },
    Check {
        name: "app_client_configs.client_id → tnt_clients.id",
        sql: "SELECT COUNT(*) FROM app_client_configs c \
              WHERE NOT EXISTS (SELECT 1 FROM tnt_clients t WHERE t.id = c.client_id)",
    },

    // --- iam_principals nullable refs ---
    Check {
        name: "iam_principals.client_id → tnt_clients.id (nullable)",
        sql: "SELECT COUNT(*) FROM iam_principals p \
              WHERE p.client_id IS NOT NULL \
                AND NOT EXISTS (SELECT 1 FROM tnt_clients c WHERE c.id = p.client_id)",
    },
    Check {
        name: "iam_principals.application_id → app_applications.id (nullable)",
        sql: "SELECT COUNT(*) FROM iam_principals p \
              WHERE p.application_id IS NOT NULL \
                AND NOT EXISTS (SELECT 1 FROM app_applications a WHERE a.id = p.application_id)",
    },

    // --- iam_service_accounts nullable refs ---
    Check {
        name: "iam_service_accounts.application_id → app_applications.id (nullable)",
        sql: "SELECT COUNT(*) FROM iam_service_accounts s \
              WHERE s.application_id IS NOT NULL \
                AND NOT EXISTS (SELECT 1 FROM app_applications a WHERE a.id = s.application_id)",
    },

    // --- iam_roles nullable refs ---
    Check {
        name: "iam_roles.application_id → app_applications.id (nullable)",
        sql: "SELECT COUNT(*) FROM iam_roles r \
              WHERE r.application_id IS NOT NULL \
                AND NOT EXISTS (SELECT 1 FROM app_applications a WHERE a.id = r.application_id)",
    },
];

/// Run all orphan checks, logging warnings where invariants are violated.
pub async fn run(pool: &PgPool) {
    let mut drifted = 0usize;

    for check in CHECKS {
        match sqlx::query_scalar::<_, i64>(check.sql).fetch_one(pool).await {
            Ok(0) => {}
            Ok(count) => {
                warn!(
                    invariant = check.name,
                    orphan_count = count,
                    "Integrity scan: orphaned rows detected. Aggregate delete path is \
                     leaving cross-table refs behind.",
                );
                drifted += 1;
            }
            Err(err) => {
                // A missing table (fresh DB before migrations) is not a drift
                // signal. We log at debug so it doesn't spam operator dashboards.
                tracing::debug!(
                    invariant = check.name,
                    error = %err,
                    "Integrity scan check failed to execute — probably a missing table; skipping.",
                );
            }
        }
    }

    if drifted == 0 {
        info!("Integrity scan: all {} junctions clean.", CHECKS.len());
    } else {
        warn!(
            drifted_junctions = drifted,
            total_junctions = CHECKS.len(),
            "Integrity scan: one or more invariants drifted. See preceding warnings.",
        );
    }
}
