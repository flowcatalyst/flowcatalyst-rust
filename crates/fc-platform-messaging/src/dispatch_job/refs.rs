//! Event type, subscription and connection filters of the dispatch-job reads
//! (Go `dispatchjob/refs.go` and `api.go` `toParams`, migration 086).
//!
//! A job's `code` IS its event type for a job the event fan-out raises (the
//! event's type) and the caller's own job code for an SDK or `fn.dispatch`
//! job, so `eventType`/`eventTypes` filter the `code` column. Subscription
//! code/name and connection id/code are resolved to ids first (one lookup on
//! the small `msg_subscriptions`/`msg_connections` tables, tenant-scoped), so
//! the job query stays a single-table read with plain `subscription_id` /
//! `connection_id` predicates. Past the caps the filter is refused (400)
//! rather than truncated: a silently truncated id list would list, count or
//! requeue a subset of what the caller named.

use sqlx::{PgPool, Postgres, QueryBuilder};

use fc_platform_core::shared::error::PlatformError;

/// The most event types one request may filter on.
pub const MAX_EVENT_TYPES: usize = 10;
/// The most subscription ids one request may filter on.
pub const MAX_SUBSCRIPTION_IDS: usize = 50;
/// Caps the subscription ids one code, name or connection resolves to.
pub const MAX_RESOLVED_SUBSCRIPTIONS: usize = 200;
/// Caps the connection ids one connection code resolves to (a code is unique
/// per client, so this is "one per client").
pub const MAX_RESOLVED_CONNECTIONS: usize = 50;
/// The 400 code for a name that resolves to more than the cap.
pub const TOO_MANY_MATCHES_CODE: &str = "TOO_MANY_MATCHES";

/// The subscription and connection references of a request, by name rather
/// than id (Go `FilterParams.Subscription*`/`Connection*`).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RefQuery {
    /// Subscription code, exact; a trailing `*` makes it a prefix.
    pub subscription_code: Option<String>,
    /// Subscription name, exact and case-insensitive; a trailing `*` makes it a prefix.
    pub subscription_name: Option<String>,
    /// Jobs delivered through this connection: those of its subscriptions and
    /// those that name it themselves (function dispatch).
    pub connection_id: Option<String>,
    /// Connection code, exact; a trailing `*` makes it a prefix.
    pub connection_code: Option<String>,
}

impl RefQuery {
    /// Whether the request names any subscription or connection by name.
    pub fn is_empty(&self) -> bool {
        self.subscription_code.is_none()
            && self.subscription_name.is_none()
            && self.connection_id.is_none()
            && self.connection_code.is_none()
    }
}

/// What a [`RefQuery`] stands for once resolved.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// Constrains `subscription_id` when a subscription code/name was given
    /// (`Some(empty)`: matched nothing, so no job matches).
    pub subs: Option<Vec<String>>,
    /// The connection constraint when a connection was named:
    /// `connection_id = ANY(ids) OR subscription_id = ANY(subs)`.
    pub connection: Option<ConnectionRefs>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ConnectionRefs {
    pub ids: Vec<String>,
    pub subscriptions: Vec<String>,
}

/// A request's references and, once [`resolve_refs`] ran, their ids. A filter
/// holding references that were not resolved matches nothing (fail closed).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Refs {
    pub query: RefQuery,
    pub resolved: Option<Resolved>,
}

impl Refs {
    pub fn new(query: RefQuery) -> Self {
        Self {
            query,
            resolved: None,
        }
    }

    /// True when a by-name reference was given and not resolved yet.
    pub fn pending(&self) -> bool {
        !self.query.is_empty() && self.resolved.is_none()
    }
}

/// Renders the resolved references as predicates (` AND ...`) on the read
/// projection. Unresolved references select nothing.
pub fn push_refs(qb: &mut QueryBuilder<'_, Postgres>, refs: &Refs) {
    if refs.pending() {
        qb.push(" AND FALSE");
        return;
    }
    let Some(r) = &refs.resolved else {
        return;
    };
    if let Some(subs) = &r.subs {
        qb.push(" AND subscription_id = ANY(")
            .push_bind(subs.clone())
            .push(")");
    }
    if let Some(c) = &r.connection {
        qb.push(" AND (connection_id = ANY(")
            .push_bind(c.ids.clone())
            .push(") OR subscription_id = ANY(")
            .push_bind(c.subscriptions.clone())
            .push("))");
    }
}

/// The filters of a request that need no lookup, validated (Go `toParams`).
#[derive(Debug, Default, Clone)]
pub struct ParsedRefs {
    pub event_types: Vec<String>,
    pub subscription_ids: Vec<String>,
    pub query: RefQuery,
}

fn split_csv(input: Option<&str>) -> Vec<String> {
    input
        .map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn trimmed(v: Option<&str>) -> Option<String> {
    v.map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// Parses `eventType`, `eventTypes`, `subscriptionIds` and the by-name
/// references. `eventType` joins `eventTypes` (at most 10: 400
/// `TOO_MANY_EVENT_TYPES`); `subscriptionIds` is at most 50 (400
/// `TOO_MANY_SUBSCRIPTION_IDS`).
pub fn parse_refs(
    event_type: Option<&str>,
    event_types: Option<&str>,
    subscription_ids: Option<&str>,
    subscription_code: Option<&str>,
    subscription_name: Option<&str>,
    connection_id: Option<&str>,
    connection_code: Option<&str>,
) -> Result<ParsedRefs, PlatformError> {
    let mut types = split_csv(event_types);
    if let Some(one) = event_type.filter(|v| !v.is_empty()) {
        if !types.iter().any(|t| t == one) {
            types.push(one.to_string());
        }
    }
    if types.len() > MAX_EVENT_TYPES {
        return Err(PlatformError::bad_request_code(
            "TOO_MANY_EVENT_TYPES",
            format!("at most {MAX_EVENT_TYPES} event types may be filtered at once"),
        ));
    }
    let ids = split_csv(subscription_ids);
    if ids.len() > MAX_SUBSCRIPTION_IDS {
        return Err(PlatformError::bad_request_code(
            "TOO_MANY_SUBSCRIPTION_IDS",
            format!("at most {MAX_SUBSCRIPTION_IDS} subscription ids may be filtered at once"),
        ));
    }
    Ok(ParsedRefs {
        event_types: types,
        subscription_ids: ids,
        query: RefQuery {
            subscription_code: trimmed(subscription_code),
            subscription_name: trimmed(subscription_name),
            connection_id: connection_id.filter(|v| !v.is_empty()).map(str::to_string),
            connection_code: trimmed(connection_code),
        },
    })
}

/// A column match for a user-typed value: a trailing `*` makes it a prefix
/// (`LIKE`, with `%`, `_` and `\` escaped), anything else is an exact match;
/// `fold` compares case-insensitively (Go `nameMatch`).
fn push_name_match(qb: &mut QueryBuilder<'_, Postgres>, col: &str, value: &str, fold: bool) {
    let prefix = value.ends_with('*');
    let v = value.strip_suffix('*').unwrap_or(value);
    if !prefix {
        if fold {
            qb.push(format!("lower({col}) = lower("))
                .push_bind(v.to_string())
                .push(")");
        } else {
            qb.push(format!("{col} = ")).push_bind(v.to_string());
        }
        return;
    }
    let escaped = format!("{}%", escape_like(v));
    if fold {
        qb.push(format!("lower({col}) LIKE lower("))
            .push_bind(escaped)
            .push(") ESCAPE '\\'");
    } else {
        qb.push(format!("{col} LIKE "))
            .push_bind(escaped)
            .push(" ESCAPE '\\'");
    }
}

fn escape_like(v: &str) -> String {
    v.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// The ids of `table`'s rows matching the `conds` builder, tenant-scoped, at
/// most `limit` of them (more is [`TOO_MANY_MATCHES_CODE`]).
async fn lookup_ids(
    pool: &PgPool,
    table: &str,
    conds: impl FnOnce(&mut QueryBuilder<'_, Postgres>),
    scope: Option<&[String]>,
    limit: usize,
    what: &str,
) -> Result<Vec<String>, PlatformError> {
    let mut qb: QueryBuilder<Postgres> =
        QueryBuilder::new(format!("SELECT id FROM {table} WHERE TRUE"));
    conds(&mut qb);
    if let Some(scope) = scope {
        qb.push(" AND (client_id IS NULL OR client_id = ANY(")
            .push_bind(scope.to_vec())
            .push("))");
    }
    qb.push(" ORDER BY id LIMIT ").push_bind(limit as i64 + 1);
    let rows: Vec<(String,)> = qb.build_query_as().fetch_all(pool).await?;
    let ids: Vec<String> = rows.into_iter().map(|(id,)| id).collect();
    if ids.len() > limit {
        return Err(PlatformError::bad_request_code(
            TOO_MANY_MATCHES_CODE,
            format!("the {what} filter matches more than {limit} records; narrow it"),
        ));
    }
    Ok(ids)
}

/// Go `ResolveRefs`: resolves a request's subscription code/name and
/// connection references to ids, so the list query stays a single-table read.
/// The lookups are scoped to the platform's own rows plus the caller's tenants
/// (`scope`: `None` is an anchor), so a client user cannot probe another
/// tenant's names.
///
/// A name that matches nothing is not an error: the filter then selects no
/// jobs. A name that matches more than the cap is a 400 `TOO_MANY_MATCHES`.
pub async fn resolve_refs(
    pool: &PgPool,
    mut refs: Refs,
    scope: Option<&[String]>,
) -> Result<Refs, PlatformError> {
    let q = refs.query.clone();
    if q.is_empty() {
        return Ok(refs);
    }
    let mut out = Resolved::default();
    if q.subscription_code.is_some() || q.subscription_name.is_some() {
        let ids = lookup_ids(
            pool,
            "msg_subscriptions",
            |qb| {
                if let Some(code) = &q.subscription_code {
                    qb.push(" AND ");
                    push_name_match(qb, "code", code, false);
                }
                if let Some(name) = &q.subscription_name {
                    qb.push(" AND ");
                    push_name_match(qb, "name", name, true);
                }
            },
            scope,
            MAX_RESOLVED_SUBSCRIPTIONS,
            "subscription",
        )
        .await?;
        out.subs = Some(ids);
    }
    if q.connection_code.is_some() || q.connection_id.is_some() {
        let ids = match (&q.connection_code, &q.connection_id) {
            (Some(code), id) => {
                let mut ids = lookup_ids(
                    pool,
                    "msg_connections",
                    |qb| {
                        qb.push(" AND ");
                        push_name_match(qb, "code", code, false);
                    },
                    scope,
                    MAX_RESOLVED_CONNECTIONS,
                    "connection",
                )
                .await?;
                if let Some(id) = id {
                    // Both given: the id must be one the code names.
                    ids.retain(|candidate| candidate == id);
                }
                ids
            }
            (None, Some(id)) => vec![id.clone()],
            (None, None) => Vec::new(),
        };
        let subscriptions = if ids.is_empty() {
            Vec::new()
        } else {
            lookup_ids(
                pool,
                "msg_subscriptions",
                |qb| {
                    qb.push(" AND connection_id = ANY(")
                        .push_bind(ids.clone())
                        .push(")");
                },
                scope,
                MAX_RESOLVED_SUBSCRIPTIONS,
                "subscription",
            )
            .await?
        };
        out.connection = Some(ConnectionRefs { ids, subscriptions });
    }
    refs.resolved = Some(out);
    Ok(refs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sql(f: impl FnOnce(&mut QueryBuilder<'_, Postgres>)) -> String {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("");
        f(&mut qb);
        qb.sql().to_string()
    }

    #[test]
    fn a_trailing_star_is_a_prefix_with_like_metacharacters_escaped() {
        assert_eq!(
            sql(|q| push_name_match(q, "code", "ord", false)),
            "code = $1"
        );
        assert_eq!(
            sql(|q| push_name_match(q, "code", "ord*", false)),
            "code LIKE $1 ESCAPE '\\'"
        );
        assert_eq!(
            sql(|q| push_name_match(q, "name", "Ord", true)),
            "lower(name) = lower($1)"
        );
        assert_eq!(
            sql(|q| push_name_match(q, "name", "Ord*", true)),
            "lower(name) LIKE lower($1) ESCAPE '\\'"
        );
        assert_eq!(escape_like(r"50%_off\x"), r"50\%\_off\\x");
    }

    #[test]
    fn event_types_join_and_are_capped_at_ten() {
        let p = parse_refs(Some("a"), Some("b, c,,"), None, None, None, None, None).unwrap();
        assert_eq!(p.event_types, ["b", "c", "a"]);
        let again = parse_refs(Some("b"), Some("b,c"), None, None, None, None, None).unwrap();
        assert_eq!(again.event_types, ["b", "c"], "no duplicate");
        let ten = (0..10)
            .map(|i| format!("t{i}"))
            .collect::<Vec<_>>()
            .join(",");
        assert!(parse_refs(None, Some(&ten), None, None, None, None, None).is_ok());
        let err =
            parse_refs(Some("one-more"), Some(&ten), None, None, None, None, None).unwrap_err();
        assert!(
            matches!(&err, PlatformError::Coded { code, .. } if code == "TOO_MANY_EVENT_TYPES"),
            "{err:?}"
        );
    }

    #[test]
    fn subscription_ids_are_capped_at_fifty() {
        let fifty = (0..50)
            .map(|i| format!("s{i}"))
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(
            parse_refs(None, None, Some(&fifty), None, None, None, None)
                .unwrap()
                .subscription_ids
                .len(),
            50
        );
        let err = parse_refs(
            None,
            None,
            Some(&format!("{fifty},x")),
            None,
            None,
            None,
            None,
        )
        .unwrap_err();
        assert!(
            matches!(&err, PlatformError::Coded { code, .. } if code == "TOO_MANY_SUBSCRIPTION_IDS"),
            "{err:?}"
        );
    }

    #[test]
    fn references_are_trimmed_and_blank_ones_dropped() {
        let p = parse_refs(
            None,
            None,
            None,
            Some(" ord* "),
            Some("  "),
            Some(""),
            Some("c1"),
        )
        .unwrap();
        assert_eq!(p.query.subscription_code.as_deref(), Some("ord*"));
        assert_eq!(p.query.subscription_name, None);
        assert_eq!(p.query.connection_id, None);
        assert_eq!(p.query.connection_code.as_deref(), Some("c1"));
    }

    #[test]
    fn unresolved_references_select_nothing_and_resolved_ones_constrain() {
        let pending = Refs::new(RefQuery {
            subscription_code: Some("x".into()),
            ..RefQuery::default()
        });
        assert!(pending.pending());
        assert_eq!(sql(|q| push_refs(q, &pending)), " AND FALSE");
        assert_eq!(sql(|q| push_refs(q, &Refs::default())), "");
        let resolved = Refs {
            query: pending.query.clone(),
            resolved: Some(Resolved {
                subs: Some(vec!["s1".into()]),
                connection: Some(ConnectionRefs {
                    ids: vec!["c1".into()],
                    subscriptions: vec!["s2".into()],
                }),
            }),
        };
        assert_eq!(
            sql(|q| push_refs(q, &resolved)),
            " AND subscription_id = ANY($1) AND (connection_id = ANY($2) OR subscription_id = ANY($3))"
        );
    }
}
