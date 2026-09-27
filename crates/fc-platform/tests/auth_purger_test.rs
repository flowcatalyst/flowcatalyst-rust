//! Go's auth purger (`StartPurger`): one sweep drops the expired short-lived
//! auth rows and keeps the live ones, and keeps the `iam_login_attempts`
//! quarterly partitions on a database Go partitioned. Requires Docker.

#[path = "support/mod.rs"]
mod support;

use chrono::{Duration, Utc};
use serde_json::json;

use fc_platform::shared::server_setup::housekeeping::{AuthPurger, RESET_TOKEN_GRACE};
use support::TestApp;

async fn ids(app: &TestApp, sql: &str) -> Vec<String> {
    let mut rows: Vec<(String,)> = sqlx::query_as(sql).fetch_all(&app.pool).await.unwrap();
    rows.sort();
    rows.into_iter().map(|(id,)| id).collect()
}

async fn exec(app: &TestApp, sql: &str) {
    sqlx::query(sql).execute(&app.pool).await.unwrap();
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn one_sweep_drops_expired_auth_rows_and_keeps_live_ones() {
    let app = TestApp::setup().await;
    let user = fc_platform::Principal::new_user("purge@x.test", fc_platform::UserScope::Anchor);
    app.repos.principal_repo.insert(&user).await.unwrap();
    let principal = user.id.clone();
    let now = Utc::now();
    let past = now - Duration::minutes(5);
    let future = now + Duration::minutes(5);

    for (id, kind, expires) in [
        ("AuthorizationCode:old", "AuthorizationCode", Some(past)),
        ("RefreshToken:old", "RefreshToken", Some(past)),
        ("Session:old", "Session", Some(past)),
        ("RefreshToken:live", "RefreshToken", Some(future)),
        ("Grant:forever", "Grant", None),
    ] {
        sqlx::query(
            "INSERT INTO oauth_oidc_payloads (id, type, payload, expires_at) VALUES ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(kind)
        .bind(json!({}))
        .bind(expires)
        .execute(&app.pool)
        .await
        .unwrap();
    }
    for (state, expires) in [("state-old", past), ("state-live", future)] {
        sqlx::query(
            "INSERT INTO oauth_oidc_login_states (state, email_domain, identity_provider_id, \
             email_domain_mapping_id, nonce, code_verifier, expires_at) \
             VALUES ($1, 'x.test', 'idp_1', 'edm_1', 'n', 'v', $2)",
        )
        .bind(state)
        .bind(expires)
        .execute(&app.pool)
        .await
        .unwrap();
    }
    for (id, expires) in [("flow-old", past), ("flow-live", future)] {
        sqlx::query(
            "INSERT INTO portal_login_flows (id, oauth_client_id, portal_client_id, redirect_uri, \
             state, expires_at) VALUES ($1, 'oc', 'clt_1', 'https://x.test/cb', 's', $2)",
        )
        .bind(id)
        .bind(expires)
        .execute(&app.pool)
        .await
        .unwrap();
    }
    for (id, expires) in [("pin-old", past), ("pin-live", future)] {
        sqlx::query(
            "INSERT INTO iam_mfa_email_pins (id, principal_id, pin_hash, expires_at) \
             VALUES ($1, $2, $1, $3)",
        )
        .bind(id)
        .bind(&principal)
        .bind(expires)
        .execute(&app.pool)
        .await
        .unwrap();
    }
    for (id, expires) in [("dev-old", past), ("dev-live", future)] {
        sqlx::query(
            "INSERT INTO iam_mfa_trusted_devices (id, principal_id, token_hash, expires_at) \
             VALUES ($1, $2, $1, $3)",
        )
        .bind(id)
        .bind(&principal)
        .bind(expires)
        .execute(&app.pool)
        .await
        .unwrap();
    }
    // A reset/invite link is kept for a grace after it expires, so a late
    // click still hears "expired".
    for (id, expires) in [
        (
            "rst-long-gone",
            now - RESET_TOKEN_GRACE - Duration::hours(1),
        ),
        ("rst-just-expired", past),
        ("rst-live", future),
    ] {
        sqlx::query(
            "INSERT INTO iam_password_reset_tokens (id, principal_id, token_hash, expires_at) \
             VALUES ($1, $2, $1, $3)",
        )
        .bind(id)
        .bind(&principal)
        .bind(expires)
        .execute(&app.pool)
        .await
        .unwrap();
    }

    let purger = AuthPurger::new(&app.pool, app.repos.oauth_client_repo.clone());
    let report = purger.run_once(now).await;

    assert_eq!(
        ids(&app, "SELECT id FROM oauth_oidc_payloads").await,
        vec!["Grant:forever", "RefreshToken:live"]
    );
    assert_eq!(
        ids(&app, "SELECT state FROM oauth_oidc_login_states").await,
        vec!["state-live"]
    );
    assert_eq!(
        ids(&app, "SELECT id FROM portal_login_flows").await,
        vec!["flow-live"]
    );
    assert_eq!(
        ids(&app, "SELECT id FROM iam_mfa_email_pins").await,
        vec!["pin-live"]
    );
    assert_eq!(
        ids(&app, "SELECT id FROM iam_mfa_trusted_devices").await,
        vec!["dev-live"]
    );
    assert_eq!(
        ids(&app, "SELECT id FROM iam_password_reset_tokens").await,
        vec!["rst-just-expired", "rst-live"]
    );
    assert_eq!(report.oauth_payloads, 3);
    assert_eq!(report.oidc_login_states, 1);
    assert_eq!(report.portal_login_flows, 1);
    assert_eq!(report.mfa, 2);
    assert_eq!(report.reset_tokens, 1);
    // Rust's own table is not partitioned: nothing to keep.
    assert!(report.dropped_partitions.is_empty());

    // A second sweep finds nothing.
    let again = purger.run_once(now).await;
    assert_eq!(
        (again.oauth_payloads, again.mfa, again.reset_tokens),
        (0, 0, 0)
    );
}

/// On a database Go's migration 049 partitioned, the sweep creates the
/// current and next quarter's partitions and drops those wholly older than
/// the three-year retention, leaving the DEFAULT partition alone.
#[tokio::test]
#[ignore = "requires Docker"]
async fn one_sweep_keeps_go_partitioned_login_attempts() {
    let app = TestApp::setup().await;
    exec(&app, "DROP TABLE iam_login_attempts").await;
    exec(
        &app,
        "CREATE TABLE iam_login_attempts (
            id VARCHAR(17) NOT NULL, attempt_type VARCHAR(30) NOT NULL,
            outcome VARCHAR(20) NOT NULL, failure_reason VARCHAR(100),
            identifier VARCHAR(255), principal_id VARCHAR(17), ip_address VARCHAR(45),
            user_agent TEXT, attempted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            PRIMARY KEY (id, attempted_at)
        ) PARTITION BY RANGE (attempted_at)",
    )
    .await;
    exec(
        &app,
        "CREATE TABLE iam_login_attempts_default PARTITION OF iam_login_attempts DEFAULT",
    )
    .await;
    exec(
        &app,
        "CREATE TABLE iam_login_attempts_2020_q1 PARTITION OF iam_login_attempts \
         FOR VALUES FROM ('2020-01-01') TO ('2020-04-01')",
    )
    .await;

    let now = chrono::DateTime::parse_from_rfc3339("2026-09-27T10:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let report = AuthPurger::new(&app.pool, app.repos.oauth_client_repo.clone())
        .run_once(now)
        .await;
    assert_eq!(
        report.dropped_partitions,
        vec!["iam_login_attempts_2020_q1"]
    );
    assert_eq!(
        ids(
            &app,
            "SELECT child.relname::text FROM pg_inherits i \
             JOIN pg_class parent ON i.inhparent = parent.oid \
             JOIN pg_class child ON i.inhrelid = child.oid \
             WHERE parent.relname = 'iam_login_attempts'"
        )
        .await,
        vec![
            "iam_login_attempts_2026_q3",
            "iam_login_attempts_2026_q4",
            "iam_login_attempts_default",
        ]
    );
    // Idempotent.
    let again = AuthPurger::new(&app.pool, app.repos.oauth_client_repo.clone())
        .run_once(now)
        .await;
    assert!(again.dropped_partitions.is_empty());
}
