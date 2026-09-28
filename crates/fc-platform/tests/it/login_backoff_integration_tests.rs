//! Login backoff — integration tests against a real PostgreSQL.
//!
//! Exercises `auth::login_backoff::check` end-to-end: insert N failures via
//! the LoginAttemptRepository, run the check, verify the decision and the
//! `Retry-After` value reflect the policy.

use chrono::{Duration, Utc};
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

use fc_platform::auth::login_backoff::{check, BackoffDecision, BackoffPolicy, BackoffReason};
use fc_platform::shared::database::{create_pool, run_migrations, MigrationProfile};
use fc_platform::{AttemptType, LoginAttempt, LoginAttemptRepository, LoginOutcome};

async fn setup_test_db() -> (sqlx::PgPool, testcontainers::ContainerAsync<Postgres>) {
    let container = Postgres::default()
        .with_db_name("flowcatalyst_test")
        .with_user("test")
        .with_password("test")
        .start()
        .await
        .expect("Failed to start PostgreSQL container");
    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();
    let url = format!("postgresql://test:test@{}:{}/flowcatalyst_test", host, port);
    let pool = create_pool(&url).await.expect("Failed to connect");
    run_migrations(&pool, MigrationProfile::Production)
        .await
        .expect("Failed to run migrations");
    (pool, container)
}

fn permissive_policy() -> BackoffPolicy {
    BackoffPolicy {
        free_attempts: 3,
        base_delay_secs: 2,
        max_delay_secs: 300,
        global_window_secs: 3600,
        global_ceiling: 100,
        global_lock_secs: 900,
    }
}

async fn record_failure(repo: &LoginAttemptRepository, email: &str, ip: &str, age: Duration) {
    let mut attempt = LoginAttempt::new(AttemptType::UserLogin, LoginOutcome::Failure);
    attempt.identifier = Some(email.to_string());
    if !ip.is_empty() {
        attempt.ip_address = Some(ip.to_string());
    }
    attempt.attempted_at = Utc::now() - age;
    repo.create(&attempt).await.expect("insert failure");
}

async fn record_success(repo: &LoginAttemptRepository, email: &str, ip: &str, age: Duration) {
    let mut attempt = LoginAttempt::new(AttemptType::UserLogin, LoginOutcome::Success);
    attempt.identifier = Some(email.to_string());
    if !ip.is_empty() {
        attempt.ip_address = Some(ip.to_string());
    }
    attempt.attempted_at = Utc::now() - age;
    repo.create(&attempt).await.expect("insert success");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn allow_when_no_history() {
    let (pool, _c) = setup_test_db().await;
    let repo = LoginAttemptRepository::new(&pool);
    let decision = check(
        &repo,
        &permissive_policy(),
        "fresh@example.com",
        Some("1.2.3.4"),
    )
    .await
    .expect("check");
    assert!(matches!(decision, BackoffDecision::Allow));
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn first_three_failures_from_same_ip_are_free() {
    let (pool, _c) = setup_test_db().await;
    let repo = LoginAttemptRepository::new(&pool);
    let policy = permissive_policy();
    let email = "alice@example.com";
    let ip = "203.0.113.5";

    for i in 0..3 {
        record_failure(&repo, email, ip, Duration::seconds(i)).await;
    }

    let decision = check(&repo, &policy, email, Some(ip)).await.expect("check");
    assert!(matches!(decision, BackoffDecision::Allow));
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn fourth_failure_triggers_pair_backoff() {
    let (pool, _c) = setup_test_db().await;
    let repo = LoginAttemptRepository::new(&pool);
    let policy = permissive_policy();
    let email = "alice@example.com";
    let ip = "203.0.113.5";

    // Four failures all just now from same (email, ip).
    for _ in 0..4 {
        record_failure(&repo, email, ip, Duration::milliseconds(100)).await;
    }

    let decision = check(&repo, &policy, email, Some(ip)).await.expect("check");
    match decision {
        BackoffDecision::Reject {
            retry_after_secs,
            reason,
        } => {
            assert_eq!(reason, BackoffReason::PairBackoff);
            // Required delay = 2s; elapsed ~0; expect ~2s remaining.
            assert!(
                (1..=2).contains(&retry_after_secs),
                "expected ~2s, got {}",
                retry_after_secs
            );
        }
        other => panic!("expected Reject, got {:?}", other),
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn legitimate_user_on_different_ip_is_not_blocked_by_attacker() {
    let (pool, _c) = setup_test_db().await;
    let repo = LoginAttemptRepository::new(&pool);
    let policy = permissive_policy();
    let email = "bob@example.com";

    // Attacker burns 20 failures from one IP — well past the per-pair backoff.
    for _ in 0..20 {
        record_failure(&repo, email, "10.0.0.1", Duration::milliseconds(500)).await;
    }

    // Legitimate user from a different IP should still be allowed.
    let decision = check(&repo, &policy, email, Some("192.168.1.50"))
        .await
        .expect("check");
    assert!(
        matches!(decision, BackoffDecision::Allow),
        "different IP should not be in backoff: {:?}",
        decision
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn distributed_attack_trips_global_ceiling() {
    let (pool, _c) = setup_test_db().await;
    let repo = LoginAttemptRepository::new(&pool);
    let mut policy = permissive_policy();
    policy.global_ceiling = 10; // tighten for testability
    let email = "victim@example.com";

    // 12 failures from 12 different IPs — each pair stays under the per-pair
    // threshold but the global ceiling trips.
    for i in 0..12 {
        let ip = format!("10.0.0.{}", i + 1);
        record_failure(&repo, email, &ip, Duration::milliseconds(100)).await;
    }

    // Even from a fresh IP, the global ceiling rejects.
    let decision = check(&repo, &policy, email, Some("203.0.113.99"))
        .await
        .expect("check");
    match decision {
        BackoffDecision::Reject {
            reason,
            retry_after_secs,
        } => {
            assert_eq!(reason, BackoffReason::GlobalCeiling);
            assert_eq!(retry_after_secs, policy.global_lock_secs as u32);
        }
        other => panic!("expected GlobalCeiling, got {:?}", other),
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn successful_login_resets_pair_backoff_window() {
    let (pool, _c) = setup_test_db().await;
    let repo = LoginAttemptRepository::new(&pool);
    let policy = permissive_policy();
    let email = "carol@example.com";
    let ip = "203.0.113.7";

    // History: 5 failures, then a success (one second ago).
    for _ in 0..5 {
        record_failure(&repo, email, ip, Duration::seconds(60)).await;
    }
    record_success(&repo, email, ip, Duration::seconds(1)).await;

    // The success cuts the failure-counting window — the next attempt
    // starts fresh.
    let decision = check(&repo, &policy, email, Some(ip)).await.expect("check");
    assert!(
        matches!(decision, BackoffDecision::Allow),
        "success should clear backoff state: {:?}",
        decision
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn empty_ip_skips_pair_check_but_still_applies_global_ceiling() {
    let (pool, _c) = setup_test_db().await;
    let repo = LoginAttemptRepository::new(&pool);
    let mut policy = permissive_policy();
    policy.global_ceiling = 5;
    let email = "noproxy@example.com";

    // 8 failures, no IP recorded (simulating no-proxy local dev).
    for _ in 0..8 {
        record_failure(&repo, email, "", Duration::milliseconds(100)).await;
    }

    // Per-pair check is skipped (empty IP), but global ceiling still trips.
    let decision = check(&repo, &policy, email, None).await.expect("check");
    assert!(
        matches!(
            decision,
            BackoffDecision::Reject {
                reason: BackoffReason::GlobalCeiling,
                ..
            }
        ),
        "expected GlobalCeiling, got {:?}",
        decision
    );
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn stale_failures_outside_window_dont_count() {
    let (pool, _c) = setup_test_db().await;
    let repo = LoginAttemptRepository::new(&pool);
    let mut policy = permissive_policy();
    policy.global_window_secs = 600; // 10 min window for the global ceiling
    policy.global_ceiling = 10;
    let email = "old@example.com";
    let ip = "203.0.113.9";

    // Old failures (1 hour ago) — outside both windows because they predate
    // the global cutoff (10 min).
    for _ in 0..20 {
        record_failure(&repo, email, ip, Duration::hours(1)).await;
    }

    // The pair check still sees them (its window is "since last success",
    // unbounded if there's no success). So expect a Reject with PairBackoff,
    // not GlobalCeiling.
    let decision = check(&repo, &policy, email, Some(ip)).await.expect("check");
    match decision {
        BackoffDecision::Reject { reason, .. } => {
            // Old failures will trigger the pair backoff but are well-past
            // their required-delay age, so the actual decision depends on
            // when the most-recent failure was. Here all are 1h old, so
            // even with N=20, elapsed (3600s) >> required (300s capped),
            // so pair check passes and global check sees them outside its
            // 10min window — net Allow.
            //
            // If we got here, that's a regression. Document it instead of
            // hard-asserting — but if we DO get here, fail with detail.
            panic!(
                "expected Allow (all failures stale), got Reject: reason={:?}",
                reason
            );
        }
        BackoffDecision::Allow => {} // expected
    }
}
