//! The application's users and its pending invitations (Go F20
//! `appusers.Repository`, internal/platform/appusers/repository.go).
//!
//! An application's users are, by definition, the CLIENT-scope users who are
//!
//! - homed at a client the application is enabled for (an enabled
//!   `app_client_configs` row), and
//! - hold at least one of the application's roles (a role named
//!   `{application code}:...`, the canonical form).
//!
//! That is the narrowest definition that is still meaningful: a user who holds
//! none of the application's roles is not the application's, an anchor or
//! partner user is a platform-level principal the application does not
//! administer, and a client the application is not enabled for is not its
//! tenant. Every read and every write goes through the same predicate
//! ([`Scope::push_where`]), so a list, a get-by-id and a resend cannot
//! disagree about who is visible.

use chrono::{DateTime, Utc};
use fc_platform_core::shared::error::Result;
use sqlx::{FromRow, PgPool, Postgres, QueryBuilder};

/// The set of users one caller may see for one application.
#[derive(Debug, Clone)]
pub struct Scope {
    pub application_id: String,
    pub application_code: String,
    /// When `Some`, narrows the visible users to those homed at one of these
    /// clients: the caller's own client reach when it is not an anchor.
    /// `None` is every client the application is enabled for.
    pub client_ids: Option<Vec<String>>,
}

impl Scope {
    /// The role-name prefix that makes a role the application's.
    fn role_prefix(&self) -> String {
        format!("{}:", self.application_code)
    }

    /// The visibility predicate, starting the `WHERE` clause. Table alias `p`
    /// is `iam_principals`.
    fn push_where(&self, qb: &mut QueryBuilder<'_, Postgres>) {
        qb.push(" WHERE p.type = 'USER' AND p.scope = 'CLIENT'");
        qb.push(
            " AND p.client_id IN (SELECT c.client_id FROM app_client_configs c \
             WHERE c.enabled AND c.application_id = ",
        )
        .push_bind(self.application_id.clone())
        .push(")");
        qb.push(
            " AND EXISTS (SELECT 1 FROM iam_principal_roles pr \
             WHERE pr.principal_id = p.id AND starts_with(pr.role_name, ",
        )
        .push_bind(self.role_prefix())
        .push("))");
        if let Some(clients) = &self.client_ids {
            qb.push(" AND p.client_id = ANY(")
                .push_bind(clients.clone())
                .push(")");
        }
    }
}

/// One application user. It carries nothing sensitive: no password hash, no
/// token, no second-factor state.
#[derive(Debug, Clone)]
pub struct User {
    pub id: String,
    pub email: String,
    pub name: String,
    pub active: bool,
    pub client_id: String,
    pub idp_type: String,
    pub last_login_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    /// The application's own roles only; the user's roles in other
    /// applications are not the application's to see.
    pub roles: Vec<String>,
    /// `PENDING` while an unexpired invite token is outstanding, `EXPIRED`
    /// for an invitee who has not set a password and has no live token, and
    /// `None` for a user who is not awaiting an invitation (a password is
    /// set, a federated login, or inactive).
    pub invitation: Option<&'static str>,
}

/// One outstanding invitation: an active, internal-password user who has not
/// yet set a password (the platform's own "password setup eligible" rule). It
/// is `PENDING` while an unexpired invite token exists and `EXPIRED`
/// otherwise (the token lapsed and was purged, or never existed), in which
/// case a resend is what the invitee needs.
#[derive(Debug, Clone)]
pub struct Invitation {
    pub principal_id: String,
    pub email: String,
    pub name: String,
    pub client_id: String,
    pub roles: Vec<String>,
    pub status: &'static str,
    /// When the latest invite was minted, or the user was created when no
    /// token remains.
    pub created_at: DateTime<Utc>,
    /// When the live token expires; `None` when none remains.
    pub expires_at: Option<DateTime<Utc>>,
}

/// The user list's filter and page.
#[derive(Debug, Clone, Default)]
pub struct ListQuery {
    pub q: String,
    pub active: Option<bool>,
    pub client_id: String,
    pub roles: Vec<String>,
    /// `name` | `email` | `createdAt`
    pub sort: String,
    pub desc: bool,
    pub page: i64,
    pub page_size: i64,
}

/// The SQL form of "awaiting an invitation": the platform's
/// password-setup-eligibility rule on `iam_principals`.
const INVITABLE: &str = "(p.active AND p.password_hash IS NULL AND p.external_idp_id IS NULL \
     AND COALESCE(p.idp_type, 'INTERNAL') <> 'OIDC')";

const LATEST_INVITE: &str =
    " LEFT JOIN (SELECT DISTINCT ON (principal_id) principal_id, created_at, expires_at \
       FROM iam_password_reset_tokens WHERE purpose = 'invite' \
       ORDER BY principal_id, created_at DESC) t ON t.principal_id = p.id";

#[derive(FromRow)]
struct UserRow {
    id: String,
    email: String,
    name: String,
    active: bool,
    client_id: String,
    idp_type: String,
    last_login_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    roles: Vec<String>,
    expires_at: Option<DateTime<Utc>>,
    awaiting: bool,
    total: i64,
}

#[derive(FromRow)]
struct InvitationRow {
    id: String,
    email: String,
    name: String,
    client_id: String,
    roles: Vec<String>,
    created_at: DateTime<Utc>,
    expires_at: Option<DateTime<Utc>>,
    total: i64,
}

fn invite_status(expires: Option<DateTime<Utc>>) -> &'static str {
    match expires {
        Some(at) if at > Utc::now() => "PENDING",
        _ => "EXPIRED",
    }
}

impl From<UserRow> for User {
    fn from(r: UserRow) -> Self {
        let invitation = r.awaiting.then(|| invite_status(r.expires_at));
        User {
            id: r.id,
            email: r.email,
            name: r.name,
            active: r.active,
            client_id: r.client_id,
            idp_type: r.idp_type,
            last_login_at: r.last_login_at,
            created_at: r.created_at,
            roles: r.roles,
            invitation,
        }
    }
}

impl From<InvitationRow> for Invitation {
    fn from(r: InvitationRow) -> Self {
        Invitation {
            principal_id: r.id,
            email: r.email,
            name: r.name,
            client_id: r.client_id,
            roles: r.roles,
            status: invite_status(r.expires_at),
            created_at: r.created_at,
            expires_at: r.expires_at,
        }
    }
}

/// Escape the LIKE metacharacters of a user-supplied search.
fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Reads the application's users and its pending invitations.
pub struct AppUsersRepository {
    pool: PgPool,
}

impl AppUsersRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    fn user_select(scope: &Scope) -> QueryBuilder<'static, Postgres> {
        let mut qb = QueryBuilder::new(
            "SELECT p.id, COALESCE(p.email, '') AS email, p.name, p.active, \
             COALESCE(p.client_id, '') AS client_id, \
             COALESCE(p.idp_type, 'INTERNAL') AS idp_type, p.last_login_at, p.created_at, \
             ARRAY(SELECT pr2.role_name FROM iam_principal_roles pr2 \
                    WHERE pr2.principal_id = p.id AND starts_with(pr2.role_name, ",
        );
        qb.push_bind(scope.role_prefix());
        qb.push(") ORDER BY pr2.role_name)::text[] AS roles, t.expires_at, ");
        qb.push(INVITABLE);
        qb.push(" AS awaiting, count(*) OVER() AS total FROM iam_principals p");
        qb.push(LATEST_INVITE);
        qb
    }

    fn invitation_select(scope: &Scope) -> QueryBuilder<'static, Postgres> {
        let mut qb = QueryBuilder::new(
            "SELECT p.id, COALESCE(p.email, '') AS email, p.name, \
             COALESCE(p.client_id, '') AS client_id, \
             ARRAY(SELECT pr2.role_name FROM iam_principal_roles pr2 \
                    WHERE pr2.principal_id = p.id AND starts_with(pr2.role_name, ",
        );
        qb.push_bind(scope.role_prefix());
        qb.push(
            ") ORDER BY pr2.role_name)::text[] AS roles, \
             COALESCE(t.created_at, p.created_at) AS created_at, t.expires_at, \
             count(*) OVER() AS total FROM iam_principals p",
        );
        qb.push(LATEST_INVITE);
        qb
    }

    /// One page of the scope's users and the total matching.
    pub async fn list(&self, scope: &Scope, q: &ListQuery) -> Result<(Vec<User>, i64)> {
        let mut qb = Self::user_select(scope);
        scope.push_where(&mut qb);
        if let Some(active) = q.active {
            qb.push(" AND p.active = ").push_bind(active);
        }
        if !q.client_id.is_empty() {
            qb.push(" AND p.client_id = ")
                .push_bind(q.client_id.clone());
        }
        if !q.q.is_empty() {
            let like = format!("%{}%", escape_like(&q.q));
            qb.push(" AND (p.name ILIKE ")
                .push_bind(like.clone())
                .push(" OR p.email ILIKE ")
                .push_bind(like)
                .push(")");
        }
        if !q.roles.is_empty() {
            qb.push(
                " AND EXISTS (SELECT 1 FROM iam_principal_roles pr3 \
                 WHERE pr3.principal_id = p.id AND pr3.role_name = ANY(",
            )
            .push_bind(q.roles.clone())
            .push("))");
        }
        let column = match q.sort.as_str() {
            "name" => "lower(p.name)",
            "email" => "lower(p.email)",
            _ => "p.created_at",
        };
        let direction = if q.desc { "DESC" } else { "ASC" };
        qb.push(format!(" ORDER BY {column} {direction}, p.id ASC LIMIT "))
            .push_bind(q.page_size)
            .push(" OFFSET ")
            .push_bind(q.page.max(0) * q.page_size);
        let rows: Vec<UserRow> = qb.build_query_as().fetch_all(&self.pool).await?;
        let total = rows.first().map_or(0, |r| r.total);
        Ok((rows.into_iter().map(User::from).collect(), total))
    }

    /// One user of the scope, or `None` when the id is not in it: an id that
    /// does not exist and an id of someone outside the scope are the same
    /// answer.
    pub async fn get(&self, scope: &Scope, id: &str) -> Result<Option<User>> {
        let mut qb = Self::user_select(scope);
        scope.push_where(&mut qb);
        qb.push(" AND p.id = ").push_bind(id.to_string());
        let row: Option<UserRow> = qb.build_query_as().fetch_optional(&self.pool).await?;
        Ok(row.map(User::from))
    }

    /// One page of the scope's outstanding invitations, newest first, and the
    /// total. `status` filters to `PENDING` or `EXPIRED` (`""` both). The
    /// token itself and its hash never leave the database.
    pub async fn invitations(
        &self,
        scope: &Scope,
        status: &str,
        page: i64,
        page_size: i64,
    ) -> Result<(Vec<Invitation>, i64)> {
        let mut qb = Self::invitation_select(scope);
        scope.push_where(&mut qb);
        qb.push(" AND ").push(INVITABLE);
        match status {
            "PENDING" => {
                qb.push(" AND t.expires_at > now()");
            }
            "EXPIRED" => {
                qb.push(" AND (t.expires_at IS NULL OR t.expires_at <= now())");
            }
            _ => {}
        }
        qb.push(" ORDER BY COALESCE(t.created_at, p.created_at) DESC, p.id ASC LIMIT ")
            .push_bind(page_size)
            .push(" OFFSET ")
            .push_bind(page.max(0) * page_size);
        let rows: Vec<InvitationRow> = qb.build_query_as().fetch_all(&self.pool).await?;
        let total = rows.first().map_or(0, |r| r.total);
        Ok((rows.into_iter().map(Invitation::from).collect(), total))
    }

    /// The scope's outstanding invitation for a user id, or `None` when the id
    /// is not an invitee in the scope (not visible, not awaiting an
    /// invitation, or not a user at all: all the same answer).
    pub async fn get_invitation(&self, scope: &Scope, id: &str) -> Result<Option<Invitation>> {
        let mut qb = Self::invitation_select(scope);
        scope.push_where(&mut qb);
        qb.push(" AND ").push(INVITABLE);
        qb.push(" AND p.id = ").push_bind(id.to_string());
        let row: Option<InvitationRow> = qb.build_query_as().fetch_optional(&self.pool).await?;
        Ok(row.map(Invitation::from))
    }

    /// Delete the principal's outstanding invite tokens; how many.
    pub async fn revoke_invites(&self, principal_id: &str) -> Result<u64> {
        let done = sqlx::query(
            "DELETE FROM iam_password_reset_tokens WHERE principal_id = $1 AND purpose = 'invite'",
        )
        .bind(principal_id)
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_metacharacters_are_escaped() {
        assert_eq!(escape_like("50%_off\\"), "50\\%\\_off\\\\");
    }

    #[test]
    fn an_invite_is_pending_only_while_unexpired() {
        assert_eq!(invite_status(None), "EXPIRED");
        assert_eq!(
            invite_status(Some(Utc::now() - chrono::Duration::hours(1))),
            "EXPIRED"
        );
        assert_eq!(
            invite_status(Some(Utc::now() + chrono::Duration::hours(1))),
            "PENDING"
        );
    }
}
