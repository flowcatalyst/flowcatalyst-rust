//! The dispatch scheduler against a real PostgreSQL: Go's claim / hold /
//! backoff model (owner decision 28, pipeline review C1, C2, H4, H5 and the
//! Medium items on the poll stall and stale recovery). Requires Docker:
//!   cargo test -p fc-platform --test it dispatch_scheduler_test:: -- --ignored

// `attribute_names`, which LocalStack 3.0 still needs.
#![allow(deprecated)]

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::support::{start_db, sync_dispatch_queue, TestDb};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use testcontainers::runners::AsyncRunner;
use testcontainers::ImageExt;
use testcontainers_modules::localstack::LocalStack;

use aws_sdk_sqs::config::Credentials;
use aws_sdk_sqs::types::MessageSystemAttributeName;
use aws_sdk_sqs::types::QueueAttributeName;
use fc_platform::dispatch_job::lifecycle;
use fc_platform::scheduler::destination::DestinationResolver;
use fc_platform::scheduler::{
    DispatchAuthService, DispatchPublisher, DispatchQueueKind, DispatchQueueSettings,
    DispatchScheduler, PoolCodeResolver, PostgresDispatchPublisher, PublishItem, PublishOutcome,
    SchedulerConfig, SqsDispatchPublisher, SubscriptionPriorityCache,
};
use fc_platform::shared::database::{create_pool, run_migrations, MigrationProfile};
use fc_queue::sqs_publisher::AwsSqsBatchApi;
use fc_queue::sqs_publisher::QueueAddressing;
use fc_queue::sqs_publisher::SqsFifoPublisher;
use std::future;
use tokio::time;
use tokio_util::sync::CancellationToken;

const APP_KEY: &str = "scheduler-test-app-key";
const ENDPOINT: &str = "http://fc-platform:8080/api/dispatch/process";

async fn setup_db() -> (PgPool, TestDb) {
    let (container, url) = start_db("fc").await;
    let pool = create_pool(&url).await.expect("connect");
    run_migrations(&pool, MigrationProfile::Production)
        .await
        .expect("migrate");
    (pool, container)
}

/// Records every published item; refuses the ids in `fail`.
#[derive(Default)]
struct RecordingPublisher {
    published: Mutex<Vec<PublishItem>>,
    fail: Mutex<HashSet<String>>,
}

#[async_trait]
impl DispatchPublisher for RecordingPublisher {
    async fn publish(&self, items: Vec<PublishItem>) -> PublishOutcome {
        let fail = self.fail.lock().unwrap().clone();
        let mut out = PublishOutcome::default();
        for item in items {
            if fail.contains(&item.job_id) {
                out.unpublished.push(item.job_id.clone());
                out.error = Some("refused".into());
            } else {
                self.published.lock().unwrap().push(item);
            }
        }
        out
    }
    fn describe(&self) -> String {
        "recording".into()
    }
}

impl RecordingPublisher {
    fn ids(&self) -> Vec<String> {
        self.published
            .lock()
            .unwrap()
            .iter()
            .map(|i| i.job_id.clone())
            .collect()
    }
}

fn scheduler(
    pool: &PgPool,
    publisher: Arc<dyn DispatchPublisher>,
    batch: usize,
) -> DispatchScheduler {
    let config = SchedulerConfig {
        batch_size: batch,
        processing_endpoint: ENDPOINT.to_string(),
        paused_cache_ttl: Duration::from_millis(0),
        ..SchedulerConfig::default()
    };
    let pool_codes = Arc::new(PoolCodeResolver::new(
        pool.clone(),
        Duration::from_millis(0),
    ));
    DispatchScheduler::new(
        config,
        pool.clone(),
        publisher,
        DispatchAuthService::from_app_key(APP_KEY).unwrap(),
        pool_codes,
    )
}

/// A job row, as fan-out or ingest would leave it.
#[derive(Clone)]
struct Job {
    id: String,
    status: &'static str,
    mode: &'static str,
    group: Option<&'static str>,
    sequence: i32,
    created_at: DateTime<Utc>,
    scheduled_for: Option<DateTime<Utc>>,
    subscription_id: Option<String>,
    client_id: Option<String>,
    dispatch_pool_id: Option<String>,
    queue: Option<&'static str>,
    updated_at: Option<DateTime<Utc>>,
}

fn job(n: i64) -> Job {
    Job {
        id: format!("j{n:012}"),
        status: "PENDING",
        mode: "NEXT_ON_ERROR",
        group: None,
        sequence: 99,
        created_at: Utc::now() - chrono::Duration::seconds(1000 - n),
        scheduled_for: None,
        subscription_id: None,
        client_id: None,
        dispatch_pool_id: None,
        queue: None,
        updated_at: None,
    }
}

async fn insert(pool: &PgPool, j: &Job) {
    sqlx::query(
        "INSERT INTO msg_dispatch_jobs (id, code, target_url, status, mode, message_group, sequence, \
         created_at, updated_at, scheduled_for, subscription_id, client_id, dispatch_pool_id, queue) \
         VALUES ($1, 'app:dom:agg:done', 'http://subscriber.test/hook', $2, $3, $4, $5, $6, \
         COALESCE($7, $6), $8, $9, $10, $11, $12)",
    )
    .bind(&j.id)
    .bind(j.status)
    .bind(j.mode)
    .bind(j.group)
    .bind(j.sequence)
    .bind(j.created_at)
    .bind(j.updated_at)
    .bind(j.scheduled_for)
    .bind(&j.subscription_id)
    .bind(&j.client_id)
    .bind(&j.dispatch_pool_id)
    .bind(j.queue)
    .execute(pool)
    .await
    .expect("insert job");
    sync_dispatch_queue(pool).await;
}

async fn status(pool: &PgPool, id: &str) -> String {
    sqlx::query_scalar("SELECT status FROM msg_dispatch_jobs WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn insert_client(pool: &PgPool, id: &str, identifier: &str) {
    sqlx::query("INSERT INTO tnt_clients (id, name, identifier) VALUES ($1, $2, $2)")
        .bind(id)
        .bind(identifier)
        .execute(pool)
        .await
        .unwrap();
}

async fn insert_subscription(
    pool: &PgPool,
    id: &str,
    queue: Option<&str>,
    connection: Option<&str>,
) {
    sqlx::query(
        "INSERT INTO msg_subscriptions (id, code, name, target, queue, connection_id) \
         VALUES ($1, $1, $1, 'http://subscriber.test/hook', $2, $3)",
    )
    .bind(id)
    .bind(queue)
    .bind(connection)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_connection(pool: &PgPool, id: &str, status: &str) {
    sqlx::query(
        "INSERT INTO msg_connections (id, code, name, status, service_account_id) \
         VALUES ($1, $1, $1, $2, 'sac_x')",
    )
    .bind(id)
    .bind(status)
    .execute(pool)
    .await
    .unwrap();
}

// ── Claim, mark QUEUED, publish, revert ─────────────────────────────────

/// A claimed job is QUEUED with `queued_at`, and its message is Go's: the
/// processing endpoint as target, a token `/process` verifies, the resolved
/// pool code, the dispatch mode and the message group.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_claimed_job_is_queued_and_its_message_is_gos() {
    let (pool, _c) = setup_db().await;
    insert_client(&pool, "clt_acme", "acme").await;
    sqlx::query(
        "INSERT INTO msg_dispatch_pools (id, code, name, client_id, client_identifier) \
         VALUES ('dpl_fast', 'FAST', 'Fast', 'clt_acme', 'acme'), ('dpl_plat', 'SHARED', 'Shared', NULL, NULL)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let mut a = job(1);
    a.group = Some("orders-1");
    a.mode = "BLOCK_ON_ERROR";
    a.client_id = Some("clt_acme".into());
    a.dispatch_pool_id = Some("dpl_fast".into());
    let mut b = job(2);
    b.client_id = Some("clt_acme".into());
    b.mode = "SOMETHING_ELSE";
    let mut c = job(3);
    c.dispatch_pool_id = Some("dpl_plat".into());
    let d = job(4);
    for j in [&a, &b, &c, &d] {
        insert(&pool, j).await;
    }

    let publisher = Arc::new(RecordingPublisher::default());
    let s = scheduler(&pool, publisher.clone(), 100);
    let report = s.poller().poll_once().await.unwrap();
    assert_eq!(report.claimed, 4);
    assert_eq!(report.published, 4);

    for j in [&a, &b, &c, &d] {
        assert_eq!(status(&pool, &j.id).await, "QUEUED");
    }
    let (queued_at,): (Option<DateTime<Utc>>,) =
        sqlx::query_as("SELECT queued_at FROM msg_dispatch_jobs WHERE id = $1")
            .bind(&a.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(queued_at.is_some());

    let auth = DispatchAuthService::from_app_key(APP_KEY).unwrap();
    let published = publisher.published.lock().unwrap().clone();
    let by_id = |id: &str| published.iter().find(|i| i.job_id == id).unwrap().clone();
    let ma = by_id(&a.id).message;
    assert_eq!(ma.mediation_target, ENDPOINT);
    assert!(auth.verify(&a.id, ma.auth_token.as_deref().unwrap()));
    assert_eq!(ma.pool_code, "acme-FAST");
    assert_eq!(ma.message_group_id.as_deref(), Some("orders-1"));
    assert_eq!(ma.dispatch_mode, fc_common::DispatchMode::BlockOnError);
    // X-01: an unknown mode is NEXT_ON_ERROR.
    let mb = by_id(&b.id).message;
    assert_eq!(mb.dispatch_mode, fc_common::DispatchMode::NextOnError);
    assert_eq!(mb.pool_code, "acme-DEFAULT-POOL");
    assert_eq!(mb.message_group_id, None);
    assert_eq!(by_id(&c.id).message.pool_code, "platform-SHARED");
    assert_eq!(by_id(&d.id).message.pool_code, "platform-DEFAULT-POOL");

    // Nothing is claimed twice: the rows are QUEUED now.
    let again = s.poller().poll_once().await.unwrap();
    assert_eq!(again.claimed, 0);
}

/// Only the jobs the publisher took are marked QUEUED; the refused ones
/// stay PENDING (nothing is reverted: they were never marked).
#[tokio::test]
#[ignore = "requires Docker"]
async fn only_published_jobs_are_marked_queued() {
    let (pool, _c) = setup_db().await;
    let (a, b) = (job(1), job(2));
    insert(&pool, &a).await;
    insert(&pool, &b).await;
    let publisher = Arc::new(RecordingPublisher::default());
    publisher.fail.lock().unwrap().insert(b.id.clone());
    let s = scheduler(&pool, publisher.clone(), 100);
    let report = s.poller().poll_once().await.unwrap();
    assert_eq!((report.claimed, report.published), (2, 1));
    assert_eq!(status(&pool, &a.id).await, "QUEUED");
    assert_eq!(status(&pool, &b.id).await, "PENDING");
}

/// Publishes, then never returns: a worker killed in the middle of its
/// publish.
struct DiesMidPublish {
    published: Mutex<Vec<String>>,
}

#[async_trait]
impl DispatchPublisher for DiesMidPublish {
    async fn publish(&self, items: Vec<PublishItem>) -> PublishOutcome {
        self.published
            .lock()
            .unwrap()
            .extend(items.into_iter().map(|i| i.job_id));
        future::pending().await
    }
    fn describe(&self) -> String {
        "dies mid-publish".into()
    }
}

/// A job the publisher refused keeps its queue row with the claim RELEASED
/// (so it is claimed again); a published job's queue row is gone.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_job_that_is_not_published_has_its_claim_released() {
    let (pool, _c) = setup_db().await;
    let (a, b) = (job(1), job(2));
    insert(&pool, &a).await;
    insert(&pool, &b).await;
    let publisher = Arc::new(RecordingPublisher::default());
    publisher.fail.lock().unwrap().insert(b.id.clone());
    let s = scheduler(&pool, publisher.clone(), 100);
    s.poller().poll_once().await.unwrap();
    let rows: Vec<(String, Option<DateTime<Utc>>)> =
        sqlx::query_as("SELECT job_id, claimed_at FROM msg_dispatch_queue ORDER BY job_id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        rows,
        vec![(b.id.clone(), None)],
        "only b is left, unclaimed"
    );
}

/// The real loop, real lanes: a job whose publish fails is claimed again and
/// published after nothing it should not be behind: the group is published
/// in order (first deliveries), every job ends QUEUED, and the queue is
/// empty and exact.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_failed_publish_is_claimed_again_and_the_group_stays_in_order() {
    let (pool, _c) = setup_db().await;
    let jobs: Vec<Job> = (1..=6)
        .map(|n| {
            let mut j = job(n);
            j.group = Some("orders-1");
            j.mode = "BLOCK_ON_ERROR";
            j.sequence = n as i32;
            j
        })
        .collect();
    for j in &jobs {
        insert(&pool, j).await;
    }
    // j3 fails the first time it is published (and with it, as the real
    // publishers do, the rest of its group in that call).
    struct FailOnce {
        inner: RecordingPublisher,
        once: Mutex<HashSet<String>>,
    }
    #[async_trait]
    impl DispatchPublisher for FailOnce {
        async fn publish(&self, items: Vec<PublishItem>) -> PublishOutcome {
            let mut out = PublishOutcome::default();
            let mut failed = false;
            for item in items {
                failed |= self.once.lock().unwrap().remove(&item.job_id);
                if failed {
                    out.unpublished.push(item.job_id.clone());
                    out.error = Some("refused once".into());
                } else {
                    self.inner.published.lock().unwrap().push(item);
                }
            }
            out
        }
        fn describe(&self) -> String {
            "fail once".into()
        }
    }
    let publisher = Arc::new(FailOnce {
        inner: RecordingPublisher::default(),
        once: Mutex::new(HashSet::from([jobs[2].id.clone()])),
    });
    let s = scheduler(&pool, publisher.clone(), 100);
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    let run = s
        .poller()
        .run(Duration::from_millis(50), Arc::new(|| true), cancel.clone());
    let watch = async {
        let deadline = time::Instant::now() + Duration::from_secs(30);
        loop {
            let (queued,): (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM msg_dispatch_jobs WHERE status = 'QUEUED'")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            if queued == 6 || time::Instant::now() > deadline {
                break;
            }
            time::sleep(Duration::from_millis(50)).await;
        }
        stop.cancel();
    };
    tokio::join!(run, watch);

    let published = publisher.inner.ids();
    let mut first_seen: Vec<String> = Vec::new();
    for id in &published {
        if !first_seen.contains(id) {
            first_seen.push(id.clone());
        }
    }
    let want: Vec<String> = jobs.iter().map(|j| j.id.clone()).collect();
    assert_eq!(first_seen, want, "published in order: {published:?}");
    for j in &jobs {
        assert_eq!(status(&pool, &j.id).await, "QUEUED");
    }
    assert!(
        sqlx::query_scalar::<_, String>("SELECT job_id FROM msg_dispatch_queue")
            .fetch_all(&pool)
            .await
            .unwrap()
            .is_empty(),
        "the queue is empty"
    );
}

/// When an instance starts polling as leader it releases every claim it does
/// not hold: what a dead leader left claimed is published.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_new_leader_releases_the_claims_a_dead_one_left_and_publishes_them() {
    let (pool, _c) = setup_db().await;
    let jobs: Vec<Job> = (1..=3).map(job).collect();
    for j in &jobs {
        insert(&pool, j).await;
    }
    sqlx::query("UPDATE msg_dispatch_queue SET claimed_at = NOW() - INTERVAL '1 hour'")
        .execute(&pool)
        .await
        .unwrap();
    let publisher = Arc::new(RecordingPublisher::default());
    let s = scheduler(&pool, publisher.clone(), 100);
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    let run = s
        .poller()
        .run(Duration::from_millis(50), Arc::new(|| true), cancel.clone());
    let watch = async {
        let deadline = time::Instant::now() + Duration::from_secs(30);
        while publisher.ids().len() < 3 && time::Instant::now() < deadline {
            time::sleep(Duration::from_millis(50)).await;
        }
        stop.cancel();
    };
    tokio::join!(run, watch);
    let mut got = publisher.ids();
    got.sort();
    let mut want: Vec<String> = jobs.iter().map(|j| j.id.clone()).collect();
    want.sort();
    assert_eq!(got, want);
}

/// The scheduler's reconcile pass repairs a PENDING job that lost its queue
/// row (only once it is old enough to be drift, not a write in flight).
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_scheduler_reconciles_a_job_that_lost_its_queue_row() {
    let (pool, _c) = setup_db().await;
    let mut old = job(1);
    old.updated_at = Some(Utc::now() - chrono::Duration::minutes(5));
    let mut fresh = job(2);
    fresh.updated_at = Some(Utc::now());
    insert(&pool, &old).await;
    insert(&pool, &fresh).await;
    sqlx::query("DELETE FROM msg_dispatch_queue")
        .execute(&pool)
        .await
        .unwrap();
    let s = scheduler(&pool, Arc::new(RecordingPublisher::default()), 100);
    let done = s.stale_recovery().reconcile_once().await.unwrap();
    assert_eq!(done.inserted, 1, "{done:?}");
    let ids: Vec<String> = sqlx::query_scalar("SELECT job_id FROM msg_dispatch_queue")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(ids, vec![old.id.clone()]);
}

/// Delivery run 3, `worker-restart`: a worker killed between claiming and
/// finishing its publish must not strand its claim. Go commits the claim
/// QUEUED before publishing and the unpublished rows wait for stale
/// recovery; here nothing is marked QUEUED until the publish is done, so the
/// dead worker's jobs are still PENDING (their queue rows claimed): a new
/// leader releases the claims it does not hold when it starts, and the next
/// poll publishes every job (the ones already sent a second time —
/// `/process` delivers each once).
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_worker_dying_mid_publish_leaves_its_claim_pending() {
    let (pool, _c) = setup_db().await;
    let jobs: Vec<Job> = (1..=3).map(job).collect();
    for j in &jobs {
        insert(&pool, j).await;
    }
    let dying = Arc::new(DiesMidPublish {
        published: Mutex::new(Vec::new()),
    });
    let s = scheduler(&pool, dying.clone(), 100);
    let poll = s.poller().poll_once();
    assert!(
        time::timeout(Duration::from_millis(500), poll)
            .await
            .is_err(),
        "the publish never returns"
    );
    assert_eq!(
        dying.published.lock().unwrap().len(),
        3,
        "it got as far as the broker"
    );
    for j in &jobs {
        assert_eq!(
            status(&pool, &j.id).await,
            "PENDING",
            "nothing stranded QUEUED"
        );
    }

    let publisher = Arc::new(RecordingPublisher::default());
    let s = scheduler(&pool, publisher.clone(), 100);
    let report = s.poller().poll_once().await.unwrap();
    assert_eq!((report.claimed, report.published), (3, 3));
    for j in &jobs {
        assert_eq!(status(&pool, &j.id).await, "QUEUED");
    }
}

/// Two schedulers polling the same table at once (a leadership overlap):
/// the claim takes no lock, so a job can be published twice, which is
/// accepted (the router drops the copy, `/process` delivers once). What must
/// hold is that no job is lost: every job is published and ends QUEUED.
#[tokio::test]
#[ignore = "requires Docker"]
async fn concurrent_schedulers_lose_no_job() {
    let (pool, _c) = setup_db().await;
    for n in 0..60 {
        insert(&pool, &job(n)).await;
    }
    let p1 = Arc::new(RecordingPublisher::default());
    let p2 = Arc::new(RecordingPublisher::default());
    let s1 = scheduler(&pool, p1.clone(), 10);
    let s2 = scheduler(&pool, p2.clone(), 10);
    for _ in 0..8 {
        let (r1, r2) = tokio::join!(s1.poller().poll_once(), s2.poller().poll_once());
        r1.unwrap();
        r2.unwrap();
    }
    let mut all = p1.ids();
    all.extend(p2.ids());
    let unique: HashSet<&String> = all.iter().collect();
    assert_eq!(unique.len(), 60, "every job published at least once");
    let (queued,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM msg_dispatch_jobs WHERE status = 'QUEUED'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(queued, 60);
}

/// The lane's QUEUED update never regresses a job the callback already moved
/// past PENDING (the router can deliver before the update runs). Drives the
/// scheduler's own statement through a publisher that, mid-publish, plays the
/// callback: one job goes PROCESSING, one COMPLETED.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_queued_update_does_not_regress_a_job_that_moved_on() {
    let (pool, _c) = setup_db().await;
    let (a, b, c) = (job(1), job(2), job(3));
    for j in [&a, &b, &c] {
        insert(&pool, j).await;
    }

    struct CallbackFirst {
        pool: PgPool,
        processing: String,
        completed: String,
    }
    #[async_trait]
    impl DispatchPublisher for CallbackFirst {
        async fn publish(&self, _items: Vec<PublishItem>) -> PublishOutcome {
            sqlx::query("UPDATE msg_dispatch_jobs SET status = 'PROCESSING' WHERE id = $1")
                .bind(&self.processing)
                .execute(&self.pool)
                .await
                .unwrap();
            sqlx::query("UPDATE msg_dispatch_jobs SET status = 'COMPLETED' WHERE id = $1")
                .bind(&self.completed)
                .execute(&self.pool)
                .await
                .unwrap();
            sync_dispatch_queue(&self.pool).await;
            PublishOutcome::default()
        }
        fn describe(&self) -> String {
            "callback first".into()
        }
    }

    let publisher = Arc::new(CallbackFirst {
        pool: pool.clone(),
        processing: a.id.clone(),
        completed: b.id.clone(),
    });
    let s = scheduler(&pool, publisher, 100);
    s.poller().poll_once().await.unwrap();
    assert_eq!(status(&pool, &a.id).await, "PROCESSING");
    assert_eq!(status(&pool, &b.id).await, "COMPLETED");
    assert_eq!(status(&pool, &c.id).await, "QUEUED");
}

/// The callback can put a job back to PENDING (a BLOCK_ON_ERROR hold, a retry
/// backoff, a deferral) between the publish and the lane's QUEUED update. The
/// update is optimistic on the row version the claim read, so that job is NOT
/// set QUEUED (it would sit QUEUED with no queue message until stale
/// recovery); it stays PENDING and is claimed again. Compile-checked only
/// where Docker is not available.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_job_rescheduled_to_pending_between_publish_and_mark_is_not_queued() {
    let (pool, _c) = setup_db().await;
    let (a, b) = (job(1), job(2));
    insert(&pool, &a).await;
    insert(&pool, &b).await;

    struct RescheduleFirst {
        pool: PgPool,
        id: String,
    }
    #[async_trait]
    impl DispatchPublisher for RescheduleFirst {
        async fn publish(&self, _items: Vec<PublishItem>) -> PublishOutcome {
            // What `reschedule` does: status PENDING, queued_at NULL, and the
            // version (`updated_at`) moves on.
            sqlx::query(
                "UPDATE msg_dispatch_jobs SET status = 'PENDING', \
                 scheduled_for = NOW() + INTERVAL '1 hour', queued_at = NULL, \
                 updated_at = NOW() + INTERVAL '1 second' WHERE id = $1",
            )
            .bind(&self.id)
            .execute(&self.pool)
            .await
            .unwrap();
            sync_dispatch_queue(&self.pool).await;
            PublishOutcome::default()
        }
        fn describe(&self) -> String {
            "reschedule first".into()
        }
    }

    let publisher = Arc::new(RescheduleFirst {
        pool: pool.clone(),
        id: a.id.clone(),
    });
    let s = scheduler(&pool, publisher, 100);
    s.poller().poll_once().await.unwrap();
    assert_eq!(status(&pool, &a.id).await, "PENDING");
    assert_eq!(status(&pool, &b.id).await, "QUEUED");
}

// ── Holds and backoff ───────────────────────────────────────────────────

/// A FAILED job holds only the BLOCK_ON_ERROR jobs behind it in its group;
/// NEXT_ON_ERROR and IMMEDIATE siblings, jobs ahead of it and other groups
/// flow (H4).
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_failure_holds_only_block_on_error_successors() {
    let (pool, _c) = setup_db().await;
    let mut ahead = job(1);
    ahead.group = Some("g");
    ahead.mode = "BLOCK_ON_ERROR";
    ahead.sequence = 1;
    let mut failed = job(2);
    failed.group = Some("g");
    failed.status = "FAILED";
    failed.sequence = 2;
    let mut boe = job(3);
    boe.group = Some("g");
    boe.mode = "BLOCK_ON_ERROR";
    boe.sequence = 3;
    let mut noe = job(4);
    noe.group = Some("g");
    noe.mode = "NEXT_ON_ERROR";
    noe.sequence = 3;
    let mut imm = job(5);
    imm.group = Some("g");
    imm.mode = "IMMEDIATE";
    imm.sequence = 3;
    let mut other = job(6);
    other.group = Some("h");
    other.mode = "BLOCK_ON_ERROR";
    // A failed ungrouped job holds nothing.
    let mut failed_ungrouped = job(7);
    failed_ungrouped.status = "FAILED";
    let mut ungrouped_boe = job(8);
    ungrouped_boe.mode = "BLOCK_ON_ERROR";
    for j in [
        &ahead,
        &failed,
        &boe,
        &noe,
        &imm,
        &other,
        &failed_ungrouped,
        &ungrouped_boe,
    ] {
        insert(&pool, j).await;
    }
    let publisher = Arc::new(RecordingPublisher::default());
    scheduler(&pool, publisher.clone(), 100)
        .poller()
        .poll_once()
        .await
        .unwrap();
    let ids: HashSet<String> = publisher.ids().into_iter().collect();
    for j in [&ahead, &noe, &imm, &other, &ungrouped_boe] {
        assert!(ids.contains(&j.id), "{} should dispatch", j.id);
    }
    assert!(!ids.contains(&boe.id), "held behind the failure");
    assert_eq!(status(&pool, &boe.id).await, "PENDING");
}

/// A job in a retry backoff is not claimed until `scheduled_for`, holds its
/// BLOCK_ON_ERROR successors meanwhile, and dispatches itself once due.
#[tokio::test]
#[ignore = "requires Docker"]
async fn a_backed_off_job_waits_and_holds_its_group_until_due() {
    let (pool, _c) = setup_db().await;
    let mut head = job(1);
    head.group = Some("g");
    head.mode = "BLOCK_ON_ERROR";
    head.sequence = 1;
    head.scheduled_for = Some(Utc::now() + chrono::Duration::seconds(60));
    let mut next = job(2);
    next.group = Some("g");
    next.mode = "BLOCK_ON_ERROR";
    next.sequence = 2;
    insert(&pool, &head).await;
    insert(&pool, &next).await;

    let publisher = Arc::new(RecordingPublisher::default());
    let s = scheduler(&pool, publisher.clone(), 100);
    assert_eq!(s.poller().poll_once().await.unwrap().claimed, 0);

    // The backoff expires: the head dispatches, and the successor with it
    // (QUEUED holds nothing; the router's FIFO keeps them in order).
    sqlx::query(
        "UPDATE msg_dispatch_jobs SET scheduled_for = NOW() - INTERVAL '1 second', \
         updated_at = NOW() WHERE id = $1",
    )
    .bind(&head.id)
    .execute(&pool)
    .await
    .unwrap();
    sync_dispatch_queue(&pool).await;
    s.poller().poll_once().await.unwrap();
    assert_eq!(publisher.ids(), vec![head.id.clone(), next.id.clone()]);
}

/// Held and paused jobs are excluded inside the claim query, so a full
/// batch of them at the head of the order cannot stall other groups (Go
/// filters after the LIMIT and does stall).
#[tokio::test]
#[ignore = "requires Docker"]
async fn held_and_paused_jobs_do_not_stall_the_rest() {
    let (pool, _c) = setup_db().await;
    insert_connection(&pool, "con_paused", "PAUSED").await;
    insert_subscription(&pool, "sub_paused", None, Some("con_paused")).await;

    let mut failed = job(1);
    failed.group = Some("a");
    failed.status = "FAILED";
    failed.sequence = 1;
    insert(&pool, &failed).await;
    for n in 2..6 {
        let mut held = job(n);
        held.group = Some("a");
        held.mode = "BLOCK_ON_ERROR";
        held.sequence = 2;
        insert(&pool, &held).await;
    }
    for n in 6..10 {
        let mut paused = job(n);
        paused.group = Some("b");
        paused.subscription_id = Some("sub_paused".into());
        insert(&pool, &paused).await;
    }
    let mut free = job(10);
    free.group = Some("z");
    insert(&pool, &free).await;

    let publisher = Arc::new(RecordingPublisher::default());
    // A batch smaller than the held + paused rows ahead of `free`.
    scheduler(&pool, publisher.clone(), 2)
        .poller()
        .poll_once()
        .await
        .unwrap();
    assert_eq!(publisher.ids(), vec![free.id.clone()]);
}

// ── Stale recovery ──────────────────────────────────────────────────────

/// QUEUED for over 15 minutes — including a row with a NULL `queued_at`,
/// as an older build inserted it (C2) — goes back to PENDING, as does a
/// PROCESSING row left for over 75 minutes whose outcome was never written.
/// Fresh rows stay, and a PROCESSING row of 30 minutes is NOT recovered (the
/// 15 minute ruling is for QUEUED only). The queue table follows.
#[tokio::test]
#[ignore = "requires Docker"]
async fn stale_recovery_returns_queued_after_15_minutes_and_processing_after_75() {
    let (pool, _c) = setup_db().await;
    let stale_queued_at = Some(Utc::now() - chrono::Duration::minutes(16));
    let fresh_queued_at = Some(Utc::now() - chrono::Duration::minutes(10));
    let stale_processing_at = Some(Utc::now() - chrono::Duration::minutes(76));
    let mid_processing_at = Some(Utc::now() - chrono::Duration::minutes(30));
    let mut stale_queued = job(1);
    stale_queued.status = "QUEUED";
    stale_queued.updated_at = stale_queued_at;
    let mut fresh_queued = job(2);
    fresh_queued.status = "QUEUED";
    fresh_queued.updated_at = fresh_queued_at;
    let mut stale_processing = job(3);
    stale_processing.status = "PROCESSING";
    stale_processing.updated_at = stale_processing_at;
    let mut mid_processing = job(4);
    mid_processing.status = "PROCESSING";
    mid_processing.updated_at = mid_processing_at;
    for j in [
        &stale_queued,
        &fresh_queued,
        &stale_processing,
        &mid_processing,
    ] {
        insert(&pool, j).await;
    }
    let s = scheduler(&pool, Arc::new(RecordingPublisher::default()), 100);
    let r = s.stale_recovery().recover_once().await.unwrap();
    assert_eq!((r.queued, r.processing), (1, 1));
    assert_eq!(status(&pool, &stale_queued.id).await, "PENDING");
    assert_eq!(status(&pool, &fresh_queued.id).await, "QUEUED");
    assert_eq!(status(&pool, &stale_processing.id).await, "PENDING");
    assert_eq!(status(&pool, &mid_processing.id).await, "PROCESSING");
    // Back in the queue, claimable.
    let queued: Vec<String> =
        sqlx::query_scalar("SELECT job_id FROM msg_dispatch_queue ORDER BY job_id")
            .fetch_all(&pool)
            .await
            .unwrap();
    let mut want = vec![stale_queued.id.clone(), stale_processing.id.clone()];
    want.sort();
    assert_eq!(queued, want);
    assert_eq!(
        lifecycle::queue_drift(&pool).await.unwrap(),
        lifecycle::QueueDrift {
            missing_or_stale: 0,
            orphaned: 0
        }
    );
}

// ── Destinations ────────────────────────────────────────────────────────

fn resolver(pool: &PgPool, settings: &DispatchQueueSettings) -> Arc<DestinationResolver> {
    Arc::new(DestinationResolver::new(
        Arc::new(PoolCodeResolver::new(
            pool.clone(),
            Duration::from_millis(0),
        )),
        SubscriptionPriorityCache::new(pool.clone(), Duration::from_millis(0)),
        settings,
    ))
}

/// The Postgres publisher writes one row per job into its tenant's queue
/// for its priority: the job's own queue wins over its subscription's, a
/// subscription's applies when the job names none, and a client-less job
/// goes to the platform tenant.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_postgres_publisher_routes_per_tenant_and_priority() {
    let (pool, _c) = setup_db().await;
    insert_client(&pool, "clt_acme", "acme").await;
    insert_subscription(&pool, "sub_high", Some("HIGH_PRIORITY"), None).await;
    let mut own = job(1);
    own.client_id = Some("clt_acme".into());
    own.subscription_id = Some("sub_high".into());
    own.queue = Some("default");
    let mut from_sub = job(2);
    from_sub.client_id = Some("clt_acme".into());
    from_sub.subscription_id = Some("sub_high".into());
    let platform = job(3);
    for j in [&own, &from_sub, &platform] {
        insert(&pool, j).await;
    }
    let settings = DispatchQueueSettings::resolve("POSTGRES", "", "", "FC-dev").unwrap();
    let publisher = Arc::new(
        PostgresDispatchPublisher::new(pool.clone(), resolver(&pool, &settings))
            .await
            .unwrap(),
    );
    let report = scheduler(&pool, publisher, 100)
        .poller()
        .poll_once()
        .await
        .unwrap();
    assert_eq!(report.published, 3);
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT id, queue_name FROM queue_messages ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        rows,
        vec![
            (own.id.clone(), "FC-dev-acme-DEFAULT".to_string()),
            (from_sub.id.clone(), "FC-dev-acme-HIGH_PRIORITY".to_string()),
            (platform.id.clone(), "FC-dev-platform-DEFAULT".to_string()),
        ]
    );
}

/// End to end on SQS (C1): the scheduler claims a job and it arrives on its
/// tenant's FIFO queue, created on first publish, with Go's body and the
/// message group as the FIFO group.
#[tokio::test]
#[ignore = "requires Docker"]
async fn the_scheduler_publishes_to_its_tenants_sqs_fifo_queue() {
    let (pool, _c) = setup_db().await;
    let localstack = LocalStack::default()
        .with_tag("3.0")
        .with_env_var("SERVICES", "sqs")
        .start()
        .await
        .expect("start localstack");
    let endpoint = format!(
        "http://{}:{}",
        localstack.get_host().await.unwrap(),
        localstack.get_host_port_ipv4(4566).await.unwrap()
    );
    let aws = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new("us-east-1"))
        .endpoint_url(&endpoint)
        .credentials_provider(Credentials::new("test", "test", None, None, "test"))
        .load()
        .await;
    let client = aws_sdk_sqs::Client::new(&aws);

    insert_client(&pool, "clt_acme", "acme").await;
    let mut j = job(1);
    j.client_id = Some("clt_acme".into());
    j.group = Some("orders-1");
    insert(&pool, &j).await;

    let settings = DispatchQueueSettings::resolve(
        "SQS",
        "https://sqs.us-east-1.amazonaws.com/000000000000/fc-dispatch.fifo",
        "",
        "FC-test",
    )
    .unwrap();
    let DispatchQueueKind::Sqs { region, account_id } = settings.kind.clone() else {
        unreachable!()
    };
    let fifo = SqsFifoPublisher::new(
        AwsSqsBatchApi::new(client.clone()),
        QueueAddressing::Composed { region, account_id },
    );
    let publisher = Arc::new(SqsDispatchPublisher::new(fifo, resolver(&pool, &settings)));
    let report = scheduler(&pool, publisher, 100)
        .poller()
        .poll_once()
        .await
        .unwrap();
    assert_eq!(report.published, 1);
    assert_eq!(status(&pool, &j.id).await, "QUEUED");

    let url = client
        .get_queue_url()
        .queue_name("FC-test-acme-DEFAULT.fifo")
        .send()
        .await
        .expect("the tenant's queue was created")
        .queue_url()
        .unwrap()
        .to_string();
    let out = client
        .receive_message()
        .queue_url(&url)
        .message_system_attribute_names(MessageSystemAttributeName::MessageGroupId)
        // LocalStack 3.0 answers the older attribute selector only.
        .attribute_names(QueueAttributeName::All)
        .wait_time_seconds(2)
        .send()
        .await
        .unwrap();
    let m = &out.messages()[0];
    let body: serde_json::Value = serde_json::from_str(m.body().unwrap()).unwrap();
    assert_eq!(body["id"], j.id.as_str());
    assert_eq!(body["mediationTarget"], ENDPOINT);
    assert_eq!(body["poolCode"], "acme-DEFAULT-POOL");
    assert_eq!(body["messageGroupId"], "orders-1");
    assert_eq!(body["dispatchMode"], "NEXT_ON_ERROR");
    assert!(body.get("signingSecret").is_none(), "Go omits empty fields");
    let auth = DispatchAuthService::from_app_key(APP_KEY).unwrap();
    assert!(auth.verify(&j.id, body["authToken"].as_str().unwrap()));
    assert_eq!(
        m.attributes()
            .and_then(|a| a.get(&MessageSystemAttributeName::MessageGroupId))
            .map(String::as_str),
        Some("orders-1")
    );
}
