//! One-off backfill: encrypt stored secrets written before encrypt-on-write.
//!
//! Every stored secret is an `encrypted:` reference from
//! [`EncryptionService::encrypt_ref`]. Rows written before that rule may still
//! hold plaintext in the columns listed in [`SECRET_COLUMNS`]. This finds
//! non-empty values without the prefix and rewrites them encrypted. It needs
//! the application key, so it can't be a SQL migration.
//!
//! ## Why this writes directly, not through a use case
//!
//! This is platform-infrastructure maintenance, like the other exceptions
//! listed in `CLAUDE.md`: it changes the storage form of a value, not its
//! meaning, and it has no executing principal. Routing it through the
//! UnitOfWork would emit a domain event and audit row per rewritten secret
//! for something that is not a business fact, and would load and re-persist
//! whole aggregates to change one column. It updates only the secret column
//! (not `updated_at`), and only where the value is still the plaintext it
//! read, so a concurrent write is never overwritten.
//!
//! It is idempotent: a second run finds nothing to do. The default is a dry
//! run that only counts. Reports carry counts, never values.

use sqlx::PgPool;

use crate::shared::encryption_service::{EncryptionService, ENCRYPTED_PREFIX};
use crate::shared::error::Result;

/// A column that holds a stored secret.
#[derive(Debug, Clone, Copy)]
pub struct SecretColumn {
    pub table: &'static str,
    pub column: &'static str,
    /// Extra condition that selects the rows whose value is a secret.
    pub row_filter: Option<&'static str>,
}

/// Every column the platform stores a secret in, except the OAuth client
/// secret (handled separately once its stored shapes are understood).
pub const SECRET_COLUMNS: [SecretColumn; 4] = [
    SecretColumn {
        table: "oauth_identity_providers",
        column: "oidc_client_secret_ref",
        row_filter: None,
    },
    SecretColumn {
        table: "iam_service_accounts",
        column: "wh_auth_token_ref",
        row_filter: None,
    },
    SecretColumn {
        table: "iam_service_accounts",
        column: "wh_signing_secret_ref",
        row_filter: None,
    },
    SecretColumn {
        table: "app_platform_configs",
        column: "value",
        row_filter: Some("value_type = 'SECRET'"),
    },
];

impl SecretColumn {
    /// `table.column`, as shown in reports.
    pub fn name(&self) -> String {
        format!("{}.{}", self.table, self.column)
    }

    /// Rows whose value is a non-empty plaintext secret. Table and column
    /// names are compile-time constants, never input.
    fn select_sql(&self) -> String {
        let filter = self
            .row_filter
            .map(|f| format!(" AND {f}"))
            .unwrap_or_default();
        format!(
            "SELECT id, {col} FROM {table} \
             WHERE {col} IS NOT NULL AND {col} <> '' \
             AND {col} NOT LIKE '{prefix}%'{filter}",
            col = self.column,
            table = self.table,
            prefix = ENCRYPTED_PREFIX,
        )
    }

    /// One batched rewrite. A row is updated only while it still holds the
    /// plaintext that was read.
    fn update_sql(&self) -> String {
        format!(
            "UPDATE {table} AS t SET {col} = v.new_value \
             FROM UNNEST($1::text[], $2::text[], $3::text[]) AS v(id, old_value, new_value) \
             WHERE t.id = v.id AND t.{col} = v.old_value",
            col = self.column,
            table = self.table,
        )
    }
}

/// What the backfill found (and, with `apply`, rewrote) in one column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnReport {
    /// `table.column`.
    pub column: String,
    /// Non-empty values without the `encrypted:` prefix.
    pub unencrypted: u64,
    /// Values rewritten encrypted. Always 0 on a dry run.
    pub encrypted: u64,
}

/// Rewrites (as parallel `ids`, `old_values`, `new_values` arrays) for a
/// set of plaintext rows. No I/O.
#[derive(Debug, Default)]
struct Rewrites {
    ids: Vec<String>,
    old_values: Vec<String>,
    new_values: Vec<String>,
}

fn plan_rewrites(enc: &EncryptionService, rows: Vec<(String, String)>) -> Result<Rewrites> {
    let mut plan = Rewrites::default();
    for (id, plaintext) in rows {
        plan.new_values.push(enc.encrypt_ref(&plaintext)?);
        plan.ids.push(id);
        plan.old_values.push(plaintext);
    }
    Ok(plan)
}

/// Find plaintext secrets in [`SECRET_COLUMNS`] and, when `apply` is true,
/// rewrite them encrypted in one transaction (one UPDATE per column).
/// Returns one report per column.
pub async fn backfill_secrets(
    pool: &PgPool,
    enc: &EncryptionService,
    apply: bool,
) -> Result<Vec<ColumnReport>> {
    let mut tx = pool.begin().await?;
    let mut reports = Vec::with_capacity(SECRET_COLUMNS.len());

    for col in SECRET_COLUMNS {
        let rows: Vec<(String, String)> = sqlx::query_as(&col.select_sql())
            .fetch_all(&mut *tx)
            .await?;
        let unencrypted = rows.len() as u64;

        let encrypted = if apply && !rows.is_empty() {
            let plan = plan_rewrites(enc, rows)?;
            sqlx::query(&col.update_sql())
                .bind(&plan.ids)
                .bind(&plan.old_values)
                .bind(&plan.new_values)
                .execute(&mut *tx)
                .await?
                .rows_affected()
        } else {
            0
        };

        reports.push(ColumnReport {
            column: col.name(),
            unencrypted,
            encrypted,
        });
    }

    if apply {
        tx.commit().await?;
    } else {
        tx.rollback().await?;
    }
    Ok(reports)
}

/// Human-readable report lines (counts only).
pub fn format_report(reports: &[ColumnReport], apply: bool) -> Vec<String> {
    let mut lines = vec![if apply {
        "Secret backfill (apply):".to_string()
    } else {
        "Secret backfill (dry run, nothing written):".to_string()
    }];
    for r in reports {
        lines.push(if apply {
            format!(
                "  {}: {} unencrypted, {} encrypted",
                r.column, r.unencrypted, r.encrypted
            )
        } else {
            format!("  {}: {} unencrypted", r.column, r.unencrypted)
        });
    }
    let pending: u64 = reports.iter().map(|r| r.unencrypted).sum();
    if !apply && pending > 0 {
        lines.push("Re-run with --apply to encrypt them.".to_string());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_encrypts_every_row_and_keeps_the_old_value_for_the_guard() {
        let enc = EncryptionService::new(&EncryptionService::generate_key()).unwrap();
        let plan = plan_rewrites(
            &enc,
            vec![
                ("idp_1".to_string(), "secret-a".to_string()),
                ("idp_2".to_string(), "secret-b".to_string()),
            ],
        )
        .unwrap();
        assert_eq!(plan.ids, ["idp_1", "idp_2"]);
        assert_eq!(plan.old_values, ["secret-a", "secret-b"]);
        for (new, old) in plan.new_values.iter().zip(&plan.old_values) {
            assert!(new.starts_with(ENCRYPTED_PREFIX));
            assert!(!new.contains(old.as_str()));
            assert_eq!(&enc.decrypt_ref(new).unwrap(), old);
        }
    }

    #[test]
    fn select_skips_empty_and_encrypted_values_and_filters_config_rows() {
        let idp = SECRET_COLUMNS[0].select_sql();
        assert!(idp.contains("FROM oauth_identity_providers"));
        assert!(idp.contains("oidc_client_secret_ref <> ''"));
        assert!(idp.contains("NOT LIKE 'encrypted:%'"));

        let cfg = SECRET_COLUMNS[3].select_sql();
        assert!(cfg.ends_with("AND value_type = 'SECRET'"));
    }

    #[test]
    fn update_is_one_batched_statement_guarded_by_the_old_value() {
        let sql = SECRET_COLUMNS[1].update_sql();
        assert!(sql.contains("UNNEST($1::text[], $2::text[], $3::text[])"));
        assert!(sql.contains("t.wh_auth_token_ref = v.old_value"));
    }

    #[test]
    fn report_has_counts_only() {
        let reports = vec![ColumnReport {
            column: "iam_service_accounts.wh_auth_token_ref".to_string(),
            unencrypted: 1,
            encrypted: 0,
        }];
        assert_eq!(
            format_report(&reports, false),
            [
                "Secret backfill (dry run, nothing written):",
                "  iam_service_accounts.wh_auth_token_ref: 1 unencrypted",
                "Re-run with --apply to encrypt them.",
            ]
        );
        let applied = vec![ColumnReport {
            encrypted: 1,
            ..reports[0].clone()
        }];
        assert_eq!(
            format_report(&applied, true)[1],
            "  iam_service_accounts.wh_auth_token_ref: 1 unencrypted, 1 encrypted"
        );
    }
}
