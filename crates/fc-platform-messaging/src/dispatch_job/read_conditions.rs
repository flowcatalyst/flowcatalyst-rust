//! The WHERE clause of every read of `msg_dispatch_jobs_read` that filters
//! like the dispatch-job list: the list itself, the requeue-by-filter match
//! count and its id walk. One builder, so the three cannot select different
//! jobs (Go builds all of them from `FilterParams.where()`; the count and walk
//! used to carry a copy of the list's conditions here).

use chrono::{DateTime, Utc};
use sqlx::{Postgres, QueryBuilder};

use super::refs::{push_refs, Refs};

/// Every condition a list-shaped read can carry. All optional; empty means
/// "no constraint". Borrowed from whichever filter struct the caller has.
#[derive(Debug, Default, Clone, Copy)]
pub struct ReadConditions<'a> {
    pub status: Option<&'a str>,
    pub statuses: &'a [String],
    /// On top of `status`/`statuses` (both must hold): the statuses a requeue
    /// may reset, so a walk never hands over a job the status guard would skip.
    pub status_in: &'a [String],
    pub client_id: Option<&'a str>,
    pub client_ids: &'a [String],
    /// Some: platform-scoped jobs and these clients' only (`None`: every job).
    pub accessible: Option<&'a [String]>,
    /// Some: only jobs whose application is one of these codes (a caller
    /// confined to its own applications, Go F20).
    pub accessible_applications: Option<&'a [String]>,
    /// Drop platform-scoped jobs (`client_id IS NULL`).
    pub exclude_platform_scoped: bool,
    pub dispatch_pool_id: Option<&'a str>,
    pub subscription_id: Option<&'a str>,
    pub code: Option<&'a str>,
    pub codes: &'a [String],
    pub source: Option<&'a str>,
    pub message_group: Option<&'a str>,
    pub applications: &'a [String],
    pub subdomains: &'a [String],
    pub aggregates: &'a [String],
    /// Jobs whose code is one of these (a job's code is its event type).
    pub event_types: &'a [String],
    /// Jobs of any of these subscriptions.
    pub subscription_ids: &'a [String],
    /// The by-name subscription and connection references, resolved to ids;
    /// unresolved ones match nothing.
    pub refs: Option<&'a Refs>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    /// Keyset cursor of an id walk: only jobs whose id sorts after it.
    pub after_id: Option<&'a str>,
}

/// Append ` AND ...` for each condition in `c` to a query that already has a
/// `WHERE` (`WHERE TRUE`).
pub fn push_read_conditions(qb: &mut QueryBuilder<'_, Postgres>, c: &ReadConditions<'_>) {
    fn eq(qb: &mut QueryBuilder<'_, Postgres>, col: &str, v: Option<&str>) {
        if let Some(v) = v {
            qb.push(format!(" AND {col} = ")).push_bind(v.to_string());
        }
    }
    fn any(qb: &mut QueryBuilder<'_, Postgres>, col: &str, v: &[String]) {
        if !v.is_empty() {
            qb.push(format!(" AND {col} = ANY("))
                .push_bind(v.to_vec())
                .push(")");
        }
    }
    eq(qb, "status", c.status);
    any(qb, "status", c.statuses);
    any(qb, "status", c.status_in);
    eq(qb, "client_id", c.client_id);
    any(qb, "client_id", c.client_ids);
    if let Some(ids) = c.accessible {
        // SECURITY: SQL-level tenant scoping. The OR group is parenthesised so
        // it ANDs with the caller's other filters.
        qb.push(" AND (client_id IS NULL OR client_id = ANY(")
            .push_bind(ids.to_vec())
            .push("))");
    }
    if let Some(codes) = c.accessible_applications {
        qb.push(" AND application = ANY(")
            .push_bind(codes.to_vec())
            .push(")");
    }
    if c.exclude_platform_scoped {
        qb.push(" AND client_id IS NOT NULL");
    }
    eq(qb, "dispatch_pool_id", c.dispatch_pool_id);
    eq(qb, "subscription_id", c.subscription_id);
    eq(qb, "code", c.code);
    any(qb, "code", c.codes);
    eq(qb, "source", c.source);
    eq(qb, "message_group", c.message_group);
    any(qb, "application", c.applications);
    any(qb, "subdomain", c.subdomains);
    any(qb, "aggregate", c.aggregates);
    any(qb, "code", c.event_types);
    any(qb, "subscription_id", c.subscription_ids);
    if let Some(refs) = c.refs {
        push_refs(qb, refs);
    }
    if let Some(t) = c.since {
        qb.push(" AND created_at >= ").push_bind(t);
    }
    if let Some(t) = c.until {
        qb.push(" AND created_at <= ").push_bind(t);
    }
    if let Some(id) = c.after_id {
        qb.push(" AND id > ").push_bind(id.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sql(c: &ReadConditions<'_>) -> String {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("SELECT 1 WHERE TRUE");
        push_read_conditions(&mut qb, c);
        qb.sql().to_string()
    }

    #[test]
    fn nothing_set_adds_nothing() {
        assert_eq!(sql(&ReadConditions::default()), "SELECT 1 WHERE TRUE");
    }

    #[test]
    fn each_condition_is_one_anded_predicate() {
        let statuses = vec!["FAILED".to_string()];
        let reach = vec!["clt_a".to_string()];
        let apps = vec!["orders".to_string()];
        let c = ReadConditions {
            status: Some("PENDING"),
            statuses: &statuses,
            status_in: &statuses,
            client_id: Some("clt_a"),
            accessible: Some(&reach),
            accessible_applications: Some(&apps),
            exclude_platform_scoped: true,
            after_id: Some("djb_1"),
            ..ReadConditions::default()
        };
        let s = sql(&c);
        for part in [
            " AND status = $1",
            " AND status = ANY($2)",
            " AND status = ANY($3)",
            " AND client_id = $4",
            " AND (client_id IS NULL OR client_id = ANY($5))",
            " AND application = ANY($6)",
            " AND client_id IS NOT NULL",
            " AND id > $7",
        ] {
            assert!(s.contains(part), "{part} in {s}");
        }
    }
}
