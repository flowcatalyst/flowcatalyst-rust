//! Audit Log Repository — PostgreSQL via SQLx

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, QueryBuilder};

use super::entity::AuditLog;
use crate::shared::error::Result;

#[derive(sqlx::FromRow)]
struct AuditLogRow {
    id: String,
    entity_type: String,
    entity_id: String,
    operation: String,
    operation_json: Option<serde_json::Value>,
    principal_id: Option<String>,
    application_id: Option<String>,
    client_id: Option<String>,
    performed_at: DateTime<Utc>,
}

impl From<AuditLogRow> for AuditLog {
    fn from(r: AuditLogRow) -> Self {
        Self {
            id: r.id,
            entity_type: r.entity_type,
            entity_id: r.entity_id,
            operation: r.operation,
            operation_json: r.operation_json,
            principal_id: r.principal_id,
            principal_name: None,
            application_id: r.application_id,
            client_id: r.client_id,
            performed_at: r.performed_at,
        }
    }
}

fn apply_audit_filters(
    qb: &mut QueryBuilder<Postgres>,
    entity_type: Option<&str>,
    entity_id: Option<&str>,
    operation: Option<&str>,
    principal_id: Option<&str>,
) {
    let mut has_where = false;
    let push_where = |qb: &mut QueryBuilder<Postgres>, has_where: &mut bool| {
        qb.push(if *has_where { " AND " } else { " WHERE " });
        *has_where = true;
    };

    if let Some(et) = entity_type {
        push_where(qb, &mut has_where);
        qb.push("entity_type = ").push_bind(et.to_string());
    }
    if let Some(eid) = entity_id {
        push_where(qb, &mut has_where);
        qb.push("entity_id = ").push_bind(eid.to_string());
    }
    if let Some(op) = operation {
        push_where(qb, &mut has_where);
        qb.push("operation = ").push_bind(op.to_string());
    }
    if let Some(pid) = principal_id {
        push_where(qb, &mut has_where);
        qb.push("principal_id = ").push_bind(pid.to_string());
    }
}

/// A stored audit row the temporary redaction sweep may rewrite: its id,
/// its operation and its `operation_json` (`None` when the stored text is
/// not a JSON document this platform can parse; such a row is left alone).
#[derive(Debug, Clone)]
pub struct AuditRedactionCandidate {
    pub id: String,
    pub operation: String,
    pub operation_json: Option<serde_json::Value>,
}

/// Case-insensitive regex matching a superset of the `operation_json` texts
/// that hold a secret-named key: every word of the redaction rule
/// (`fc_common::audit_redaction`), with `_` or `-` allowed between any two
/// letters because the rule strips them before matching. Built from the
/// rule's own word lists, so the two cannot drift.
fn secret_key_pattern() -> String {
    use fc_common::audit_redaction::{SECRET_EXACT, SECRET_SUFFIXES};
    SECRET_SUFFIXES
        .iter()
        .chain(SECRET_EXACT)
        .map(|word| {
            word.chars()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join("[_-]*")
        })
        .collect::<Vec<_>>()
        .join("|")
}

pub struct AuditLogRepository {
    pool: PgPool,
}

impl AuditLogRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn insert(&self, log: &AuditLog) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO aud_logs
                (id, entity_type, entity_id, operation, operation_json,
                 principal_id, application_id, client_id, performed_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW())"#,
        )
        .bind(&log.id)
        .bind(&log.entity_type)
        .bind(&log.entity_id)
        .bind(&log.operation)
        .bind(&log.operation_json)
        .bind(&log.principal_id)
        .bind(&log.application_id)
        .bind(&log.client_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_by_id(&self, id: &str) -> Result<Option<AuditLog>> {
        let row = sqlx::query_as::<_, AuditLogRow>("SELECT * FROM aud_logs WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(AuditLog::from))
    }

    pub async fn find_by_entity(
        &self,
        entity_type: &str,
        entity_id: &str,
        limit: i64,
    ) -> Result<Vec<AuditLog>> {
        let rows = sqlx::query_as::<_, AuditLogRow>(
            "SELECT * FROM aud_logs WHERE entity_type = $1 AND entity_id = $2 \
             ORDER BY performed_at DESC LIMIT $3",
        )
        .bind(entity_type)
        .bind(entity_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(AuditLog::from).collect())
    }

    pub async fn find_by_principal(&self, principal_id: &str, limit: i64) -> Result<Vec<AuditLog>> {
        let rows = sqlx::query_as::<_, AuditLogRow>(
            "SELECT * FROM aud_logs WHERE principal_id = $1 ORDER BY performed_at DESC LIMIT $2",
        )
        .bind(principal_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(AuditLog::from).collect())
    }

    pub async fn find_recent(&self, limit: i64) -> Result<Vec<AuditLog>> {
        let rows = sqlx::query_as::<_, AuditLogRow>(
            "SELECT * FROM aud_logs ORDER BY performed_at DESC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(AuditLog::from).collect())
    }

    pub async fn search(
        &self,
        entity_type: Option<&str>,
        entity_id: Option<&str>,
        operation: Option<&str>,
        principal_id: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<AuditLog>> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("SELECT * FROM aud_logs");
        apply_audit_filters(&mut qb, entity_type, entity_id, operation, principal_id);
        qb.push(" ORDER BY performed_at DESC LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);

        let rows: Vec<AuditLogRow> = qb.build_query_as().fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(AuditLog::from).collect())
    }

    pub async fn count_with_filters(
        &self,
        entity_type: Option<&str>,
        entity_id: Option<&str>,
        operation: Option<&str>,
        principal_id: Option<&str>,
    ) -> Result<i64> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("SELECT COUNT(*) FROM aud_logs");
        apply_audit_filters(&mut qb, entity_type, entity_id, operation, principal_id);
        let count: i64 = qb.build_query_scalar().fetch_one(&self.pool).await?;
        Ok(count)
    }

    /// Cursor-paginated search. Keyset on `(performed_at, id) DESC`. Returns
    /// `fetch_limit` rows so the caller can detect `hasMore`.
    pub async fn search_with_cursor(
        &self,
        entity_type: Option<&str>,
        entity_id: Option<&str>,
        operation: Option<&str>,
        principal_id: Option<&str>,
        cursor: Option<&crate::shared::api_common::DecodedCursor>,
        fetch_limit: i64,
    ) -> Result<Vec<AuditLog>> {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("SELECT * FROM aud_logs");
        apply_audit_filters(&mut qb, entity_type, entity_id, operation, principal_id);
        if let Some(c) = cursor {
            // apply_audit_filters injects WHERE/AND for any filter; if there
            // were none we need WHERE here, otherwise AND.
            let already_has_where = entity_type.is_some()
                || entity_id.is_some()
                || operation.is_some()
                || principal_id.is_some();
            qb.push(if already_has_where {
                " AND "
            } else {
                " WHERE "
            });
            qb.push("(performed_at, id) < (")
                .push_bind(c.created_at)
                .push(", ")
                .push_bind(c.id.clone())
                .push(")");
        }
        qb.push(" ORDER BY performed_at DESC, id DESC LIMIT ")
            .push_bind(fetch_limit);
        let rows: Vec<AuditLogRow> = qb.build_query_as().fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(AuditLog::from).collect())
    }

    pub async fn find_distinct_entity_types(&self) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT entity_type FROM aud_logs ORDER BY entity_type",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn find_distinct_application_ids(&self) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT application_id FROM aud_logs \
             WHERE application_id IS NOT NULL ORDER BY application_id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn find_distinct_client_ids(&self) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT client_id FROM aud_logs \
             WHERE client_id IS NOT NULL ORDER BY client_id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// **Temporary** (owner spec `docs/spec/audit-redaction.md` in the Java
    /// repo, "Temporary: redact existing rows from the dashboard"; remove with
    /// the sweep): the next `limit` rows after `after_id`, in id order, that
    /// may hold a secret — a superset filtered in SQL so the sweep never
    /// reads the whole table in one request. A row is a candidate when its
    /// `operation` is one of `masked_operations` (a command with declared
    /// masked fields) or its `operation_json` text matches a secret-key word.
    /// The caller applies the exact rule.
    pub async fn find_redaction_candidates(
        &self,
        after_id: &str,
        masked_operations: &[&str],
        limit: i64,
    ) -> Result<Vec<AuditRedactionCandidate>> {
        let rows = sqlx::query_as::<_, (String, String, Option<String>)>(
            r#"SELECT id, operation, operation_json::text
               FROM aud_logs
               WHERE id > $1
                 AND operation_json IS NOT NULL
                 AND (operation = ANY($2) OR operation_json::text ~* $3)
               ORDER BY id
               LIMIT $4"#,
        )
        .bind(after_id)
        .bind(masked_operations)
        .bind(secret_key_pattern())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(id, operation, text)| AuditRedactionCandidate {
                id,
                operation,
                operation_json: text.and_then(|t| serde_json::from_str(&t).ok()),
            })
            .collect())
    }

    /// **Temporary** (see [`Self::find_redaction_candidates`]): replace the
    /// `operation_json` of each `(id, document)` in one statement. Audit rows
    /// are otherwise append-only; this is the sweep's platform maintenance
    /// write, not a business operation.
    pub async fn rewrite_operation_json(
        &self,
        rows: &[(String, serde_json::Value)],
    ) -> Result<u64> {
        if rows.is_empty() {
            return Ok(0);
        }
        let ids: Vec<&str> = rows.iter().map(|(id, _)| id.as_str()).collect();
        let docs: Vec<String> = rows.iter().map(|(_, doc)| doc.to_string()).collect();
        let result = sqlx::query(
            r#"UPDATE aud_logs AS a
               SET operation_json = u.doc::jsonb
               FROM UNNEST($1::varchar[], $2::text[]) AS u(id, doc)
               WHERE a.id = u.id"#,
        )
        .bind(&ids)
        .bind(&docs)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn find_distinct_operations(&self) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT operation FROM aud_logs ORDER BY operation",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_common::audit_redaction::redact;
    use serde_json::json;

    /// The SQL pre-filter keeps every document the exact rule changes:
    /// Postgres `~*` is a case-insensitive POSIX regex, which the `regex`
    /// crate's `(?i)` matches for this pattern's plain letters and classes.
    #[test]
    fn the_pre_filter_is_a_superset_of_the_rule() {
        let pre_filter = regex::Regex::new(&format!("(?i){}", secret_key_pattern())).unwrap();
        let secret_docs = [
            json!({"password": "x"}),
            json!({"Pass_Word": "x"}),
            json!({"newPassword": "x"}),
            json!({"x": {"CLIENT-SECRET": "x"}}),
            json!({"oidcClientSecretRef": "x"}),
            json!({"passphrase": "x"}),
            json!({"list": [{"refresh_token": "x"}]}),
            json!({"api_key": "x"}),
            json!({"a-p-i-k-e-y": "x"}),
            json!({"privateKey": "x"}),
            json!({"Authorization": "x"}),
            json!({"cookie": "x"}),
        ];
        for doc in secret_docs {
            assert_ne!(redact(&doc, &[]), doc, "the rule should change {doc}");
            assert!(
                pre_filter.is_match(&doc.to_string()),
                "pre-filter missed {doc}"
            );
        }
        // Look-alikes may match (it is a superset); the rule decides.
        assert!(!pre_filter.is_match(&json!({"name": "kept", "value": 1}).to_string()));
    }
}
