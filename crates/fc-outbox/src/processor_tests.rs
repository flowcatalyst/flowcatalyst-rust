//! The processor end to end against a real (in-memory SQLite) outbox table,
//! with the platform replaced by a scripted dispatcher.

use crate::enhanced_processor::{EnhancedOutboxProcessor, EnhancedProcessorConfig};
use crate::http_dispatcher::{DispatchOutcome, OutboxDispatcher};
use crate::repository::OutboxRepository;
use crate::sqlite::tests::{insert, repo, row};
use crate::sqlite::SqliteOutboxRepository;
use crate::GroupStatus;
use async_trait::async_trait;
use fc_common::{OutboxItem, OutboxStatus};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Semaphore;

type Script = Box<dyn Fn(&OutboxItem) -> DispatchOutcome + Send + Sync>;

/// The platform: answers each item by `script`, records every request.
struct Platform {
    script: Script,
    requests: Mutex<Vec<Vec<String>>>,
    /// When set, each request waits for a permit (to hold items in flight).
    gate: Option<Arc<Semaphore>>,
}

impl Platform {
    fn new(script: impl Fn(&OutboxItem) -> DispatchOutcome + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            script: Box::new(script),
            requests: Mutex::default(),
            gate: None,
        })
    }

    fn accepting() -> Arc<Self> {
        Self::new(|_| DispatchOutcome::success())
    }

    fn requests(&self) -> Vec<Vec<String>> {
        self.requests.lock().unwrap().clone()
    }

    fn sent(&self) -> Vec<String> {
        self.requests().into_iter().flatten().collect()
    }
}

#[async_trait]
impl OutboxDispatcher for Platform {
    async fn send_batch(&self, items: &[OutboxItem]) -> Vec<DispatchOutcome> {
        if let Some(gate) = &self.gate {
            gate.acquire().await.unwrap().forget();
        }
        self.requests
            .lock()
            .unwrap()
            .push(items.iter().map(|i| i.id.clone()).collect());
        items.iter().map(|i| (self.script)(i)).collect()
    }
}

fn config() -> EnhancedProcessorConfig {
    EnhancedProcessorConfig::default()
}

async fn setup(
    platform: Arc<Platform>,
    config: EnhancedProcessorConfig,
) -> (EnhancedOutboxProcessor, Arc<SqliteOutboxRepository>) {
    let repo = Arc::new(repo().await);
    let processor = EnhancedOutboxProcessor::with_dispatcher(
        config,
        repo.clone() as Arc<dyn OutboxRepository>,
        platform,
    );
    (processor, repo)
}

/// Polls once and waits for everything it handed out to finish.
async fn poll(p: &EnhancedOutboxProcessor) {
    p.poll_once().await.unwrap();
    settle(p).await;
}

async fn settle(p: &EnhancedOutboxProcessor) {
    for _ in 0..400 {
        if p.in_flight_count() == 0 && p.distributor_stats().active_groups == 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("processor did not settle");
}

async fn add(
    repo: &SqliteOutboxRepository,
    id: &str,
    item_type: &str,
    group: Option<&str>,
    at: u32,
) {
    insert(
        repo,
        id,
        item_type,
        group,
        &format!(r#"{{"id":"{id}"}}"#),
        &format!("2026-01-01 00:00:{at:02}"),
    )
    .await;
}

fn fail(status: OutboxStatus) -> DispatchOutcome {
    DispatchOutcome::failed(status, format!("{status:?}"))
}

#[tokio::test]
async fn an_accepted_row_is_deleted_and_nothing_is_sent_twice() {
    let platform = Platform::accepting();
    let (p, repo) = setup(platform.clone(), config()).await;
    add(&repo, "e1", "EVENT", None, 1).await;
    add(&repo, "d1", "DISPATCH_JOB", None, 2).await;
    add(&repo, "a1", "AUDIT_LOG", None, 3).await;

    poll(&p).await;
    for id in ["e1", "d1", "a1"] {
        assert_eq!(row(&repo, id).await, None, "{id}");
    }
    // One request per type for ungrouped items.
    assert_eq!(platform.requests().len(), 3);

    poll(&p).await;
    assert_eq!(platform.sent().len(), 3, "every type is claimed once");
}

#[tokio::test]
async fn ungrouped_items_of_a_type_share_one_request() {
    let platform = Platform::accepting();
    let (p, repo) = setup(platform.clone(), config()).await;
    for i in 0..5 {
        add(&repo, &format!("e{i}"), "EVENT", None, i).await;
    }
    poll(&p).await;
    assert_eq!(platform.requests().len(), 1);
    assert_eq!(platform.requests()[0].len(), 5);
}

#[tokio::test]
async fn a_row_is_not_deleted_before_the_platform_answers() {
    let platform = Arc::new(Platform {
        script: Box::new(|_| DispatchOutcome::success()),
        requests: Mutex::default(),
        gate: Some(Arc::new(Semaphore::new(0))),
    });
    let (p, repo) = setup(platform.clone(), config()).await;
    add(&repo, "g1", "EVENT", Some("g"), 1).await;
    add(&repo, "u1", "EVENT", None, 2).await;

    p.poll_once().await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    // In flight: claimed, still in the table.
    assert_eq!(row(&repo, "g1").await.unwrap().0, 9);
    assert_eq!(row(&repo, "u1").await.unwrap().0, 9);
    assert_eq!(p.in_flight_count(), 2);

    // The process "dies" here: nothing it held is lost. Recovery returns the
    // rows to PENDING for the next processor.
    drop(p);
    sqlx::query("UPDATE outbox_messages SET updated_at = '2020-01-01T00:00:00.000Z'")
        .execute(repo.pool())
        .await
        .unwrap();
    assert_eq!(
        repo.recover_stuck(Duration::from_secs(300)).await.unwrap(),
        2
    );
    assert_eq!(row(&repo, "g1").await.unwrap().0, 0);
    assert_eq!(row(&repo, "u1").await.unwrap().0, 0);
}

#[tokio::test]
async fn a_retryable_failure_is_retried_then_kept_failed() {
    let platform = Platform::new(|_| fail(OutboxStatus::GatewayError));
    let (p, repo) = setup(platform.clone(), config()).await;
    add(&repo, "e1", "EVENT", None, 1).await;

    poll(&p).await;
    assert_eq!(
        row(&repo, "e1").await,
        Some((0, 1, Some("GatewayError".into())))
    );
    poll(&p).await;
    assert_eq!(row(&repo, "e1").await.unwrap().0, 0);
    // The third attempt is the last (max_retries 3): the row keeps its
    // failure status and isn't claimed again.
    poll(&p).await;
    assert_eq!(
        row(&repo, "e1").await,
        Some((6, 3, Some("GatewayError".into())))
    );
    poll(&p).await;
    assert_eq!(platform.sent().len(), 3);
}

#[tokio::test]
async fn a_terminal_failure_is_kept_at_once() {
    for (status, code) in [(OutboxStatus::Forbidden, 5), (OutboxStatus::BadRequest, 2)] {
        let platform = Platform::new(move |_| fail(status));
        let (p, repo) = setup(platform.clone(), config()).await;
        add(&repo, "d1", "DISPATCH_JOB", None, 1).await;
        poll(&p).await;
        assert_eq!(row(&repo, "d1").await.unwrap().0, code);
        assert_eq!(row(&repo, "d1").await.unwrap().1, 1);
        poll(&p).await;
        assert_eq!(platform.sent().len(), 1);
    }
}

#[tokio::test]
async fn per_item_outcomes_are_recorded_per_row() {
    let platform = Platform::new(|item| match item.id.as_str() {
        "ok" => DispatchOutcome::success(),
        "bad" => DispatchOutcome::failed(OutboxStatus::BadRequest, "unknown event type"),
        _ => fail(OutboxStatus::InternalError),
    });
    let (p, repo) = setup(platform, config()).await;
    add(&repo, "ok", "EVENT", None, 1).await;
    add(&repo, "bad", "EVENT", None, 2).await;
    add(&repo, "retry", "EVENT", None, 3).await;
    poll(&p).await;
    assert_eq!(row(&repo, "ok").await, None);
    assert_eq!(
        row(&repo, "bad").await,
        Some((2, 1, Some("unknown event type".into())))
    );
    assert_eq!(row(&repo, "retry").await.unwrap().0, 0);
}

#[tokio::test]
async fn a_group_is_sent_in_order_one_item_at_a_time() {
    let platform = Platform::accepting();
    let (p, repo) = setup(platform.clone(), config()).await;
    // Inserted out of order; created_at decides.
    add(&repo, "g3", "EVENT", Some("g"), 3).await;
    add(&repo, "g1", "EVENT", Some("g"), 1).await;
    add(&repo, "g2", "DISPATCH_JOB", Some("g"), 2).await;
    poll(&p).await;
    assert_eq!(
        platform.requests(),
        vec![vec!["g1".to_string()], vec!["g2".into()], vec!["g3".into()]]
    );
}

#[tokio::test]
async fn a_retryable_failure_stops_the_group_without_blocking_it() {
    let failures = Arc::new(Mutex::new(1));
    let f = failures.clone();
    let platform = Platform::new(move |item| {
        let mut left = f.lock().unwrap();
        if item.id == "g1" && *left > 0 {
            *left -= 1;
            return fail(OutboxStatus::GatewayError);
        }
        DispatchOutcome::success()
    });
    let (p, repo) = setup(platform.clone(), config()).await;
    for (i, id) in ["g1", "g2", "g3"].iter().enumerate() {
        add(&repo, id, "EVENT", Some("g"), i as u32).await;
    }

    poll(&p).await;
    // g1 failed and is PENDING again; g2 and g3 were released unsent.
    assert_eq!(platform.sent(), vec!["g1"]);
    assert_eq!(
        row(&repo, "g1").await.unwrap(),
        (0, 1, Some("GatewayError".into()))
    );
    assert_eq!(row(&repo, "g2").await.unwrap(), (0, 0, None));
    assert!(p.blocked_groups().is_empty());

    poll(&p).await;
    assert_eq!(platform.sent(), vec!["g1", "g1", "g2", "g3"]);
    assert_eq!(row(&repo, "g3").await, None);
}

#[tokio::test]
async fn a_final_failure_blocks_the_group_until_unblocked() {
    let refuse = Arc::new(Mutex::new(true));
    let r = refuse.clone();
    let platform = Platform::new(move |item| {
        if item.id == "g1" && *r.lock().unwrap() {
            return fail(OutboxStatus::Forbidden);
        }
        DispatchOutcome::success()
    });
    let (p, repo) = setup(platform.clone(), config()).await;
    add(&repo, "g1", "DISPATCH_JOB", Some("g"), 1).await;
    add(&repo, "g2", "DISPATCH_JOB", Some("g"), 2).await;
    add(&repo, "h1", "DISPATCH_JOB", Some("h"), 3).await;

    poll(&p).await;
    let blocked = p.blocked_groups();
    assert_eq!(blocked.len(), 1);
    assert_eq!(blocked[0].group, "g");
    assert_eq!(blocked[0].status, GroupStatus::Blocked);
    assert_eq!(blocked[0].blocked_item_id, "g1");
    assert_eq!(row(&repo, "g1").await.unwrap().0, 5);
    // Other groups are unaffected.
    assert_eq!(row(&repo, "h1").await, None);

    // While blocked, the group's items are claimed and released, never sent.
    poll(&p).await;
    poll(&p).await;
    assert_eq!(row(&repo, "g2").await.unwrap(), (0, 0, None));
    assert_eq!(platform.sent(), vec!["g1", "h1"]);

    // Unblock re-queues the poison item: the group runs again in order.
    *refuse.lock().unwrap() = false;
    assert!(p.unblock_group("g").await);
    assert_eq!(row(&repo, "g1").await.unwrap(), (0, 0, None));
    poll(&p).await;
    assert_eq!(platform.sent(), vec!["g1", "h1", "g1", "g2"]);
    assert_eq!(row(&repo, "g1").await, None);
    assert_eq!(row(&repo, "g2").await, None);
    assert!(!p.unblock_group("g").await, "no longer blocked");
}

#[tokio::test]
async fn skip_advances_past_the_poison_item() {
    let platform = Platform::new(|item| {
        if item.id == "g1" {
            fail(OutboxStatus::BadRequest)
        } else {
            DispatchOutcome::success()
        }
    });
    let (p, repo) = setup(platform.clone(), config()).await;
    add(&repo, "g1", "EVENT", Some("g"), 1).await;
    add(&repo, "g2", "EVENT", Some("g"), 2).await;
    poll(&p).await;
    assert!(p.skip_group("g"));
    poll(&p).await;
    assert_eq!(row(&repo, "g1").await.unwrap().0, 2, "stays failed");
    assert_eq!(row(&repo, "g2").await, None);
    assert!(!p.skip_group("g"));
}

#[tokio::test]
async fn retries_exhausted_block_the_group() {
    let platform = Platform::new(|_| fail(OutboxStatus::InternalError));
    let (p, repo) = setup(platform.clone(), config()).await;
    add(&repo, "g1", "EVENT", Some("g"), 1).await;
    add(&repo, "g2", "EVENT", Some("g"), 2).await;
    for _ in 0..3 {
        assert!(p.blocked_groups().is_empty());
        poll(&p).await;
    }
    assert_eq!(p.blocked_groups().len(), 1);
    assert_eq!(
        row(&repo, "g1").await.unwrap(),
        (3, 3, Some("InternalError".into()))
    );
    assert_eq!(platform.sent(), vec!["g1", "g1", "g1"]);
}

#[tokio::test]
async fn without_block_on_error_a_group_moves_on() {
    let platform = Platform::new(|item| {
        if item.id == "g1" {
            fail(OutboxStatus::Forbidden)
        } else {
            DispatchOutcome::success()
        }
    });
    let config = EnhancedProcessorConfig {
        block_on_error: false,
        ..config()
    };
    let (p, repo) = setup(platform.clone(), config).await;
    add(&repo, "g1", "EVENT", Some("g"), 1).await;
    add(&repo, "g2", "EVENT", Some("g"), 2).await;
    poll(&p).await;
    assert_eq!(platform.sent(), vec!["g1", "g2"]);
    assert!(p.blocked_groups().is_empty());
    assert_eq!(row(&repo, "g2").await, None);
}

#[tokio::test]
async fn a_paused_group_is_released_until_resumed() {
    let platform = Platform::accepting();
    let (p, repo) = setup(platform.clone(), config()).await;
    add(&repo, "g1", "EVENT", Some("g"), 1).await;
    p.pause_group("g");
    assert_eq!(p.group_states()[0].status, GroupStatus::Paused);
    poll(&p).await;
    assert!(platform.sent().is_empty());
    assert_eq!(row(&repo, "g1").await.unwrap(), (0, 0, None));
    p.resume_group("g");
    poll(&p).await;
    assert_eq!(row(&repo, "g1").await, None);
}

#[tokio::test]
async fn an_unreadable_payload_fails_terminally_and_blocks_its_group() {
    let platform = Platform::accepting();
    let (p, repo) = setup(platform.clone(), config()).await;
    insert(
        &repo,
        "bad",
        "EVENT",
        Some("g"),
        "{not json",
        "2026-01-01 00:00:01",
    )
    .await;
    add(&repo, "g2", "EVENT", Some("g"), 2).await;
    add(&repo, "ok", "EVENT", None, 3).await;
    poll(&p).await;
    let (status, retries, error) = row(&repo, "bad").await.unwrap();
    assert_eq!((status, retries), (2, 1));
    assert!(error.unwrap().contains("not JSON"));
    assert_eq!(p.blocked_groups()[0].blocked_item_id, "bad");
    assert_eq!(row(&repo, "g2").await.unwrap().0, 0, "released behind it");
    assert_eq!(
        row(&repo, "ok").await,
        None,
        "the rest of the claim is sent"
    );
}

#[tokio::test]
async fn no_poll_while_max_in_flight_is_reached() {
    let platform = Arc::new(Platform {
        script: Box::new(|_| DispatchOutcome::success()),
        requests: Mutex::default(),
        gate: Some(Arc::new(Semaphore::new(0))),
    });
    let config = EnhancedProcessorConfig {
        max_in_flight: 2,
        ..config()
    };
    let (p, repo) = setup(platform.clone(), config).await;
    for i in 0..3 {
        add(&repo, &format!("e{i}"), "EVENT", None, i).await;
    }
    let gate = platform.gate.clone().unwrap();

    p.poll_once().await.unwrap();
    assert_eq!(p.in_flight_count(), 3);
    // At the limit: the next poll claims nothing.
    add(&repo, "late", "EVENT", None, 9).await;
    p.poll_once().await.unwrap();
    assert_eq!(row(&repo, "late").await.unwrap().0, 0);

    gate.add_permits(10);
    settle(&p).await;
    poll(&p).await;
    assert_eq!(row(&repo, "late").await, None);
}

#[tokio::test]
async fn a_row_recovered_while_still_held_is_not_sent_twice() {
    let gate = Arc::new(Semaphore::new(0));
    let platform = Arc::new(Platform {
        script: Box::new(|_| DispatchOutcome::success()),
        requests: Mutex::default(),
        gate: Some(gate.clone()),
    });
    let (p, repo) = setup(platform.clone(), config()).await;
    add(&repo, "e1", "EVENT", None, 1).await;
    p.poll_once().await.unwrap();

    // Stuck past the threshold while this process still holds it.
    sqlx::query("UPDATE outbox_messages SET updated_at = '2020-01-01T00:00:00.000Z'")
        .execute(repo.pool())
        .await
        .unwrap();
    assert_eq!(p.recover_once().await.unwrap(), 1);
    p.poll_once().await.unwrap();

    gate.add_permits(10);
    settle(&p).await;
    assert_eq!(platform.sent(), vec!["e1"]);
    assert_eq!(row(&repo, "e1").await, None);
}

#[tokio::test]
async fn start_polls_until_stopped() {
    let platform = Platform::accepting();
    let config = EnhancedProcessorConfig {
        poll_interval: Duration::from_millis(10),
        ..config()
    };
    let (p, repo) = setup(platform.clone(), config).await;
    let p = Arc::new(p);
    add(&repo, "e1", "EVENT", None, 1).await;
    let runner = {
        let p = p.clone();
        tokio::spawn(async move { p.start().await })
    };
    for _ in 0..200 {
        if row(&repo, "e1").await.is_none() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(row(&repo, "e1").await, None);
    p.stop();
    tokio::time::timeout(Duration::from_secs(2), runner)
        .await
        .unwrap()
        .unwrap();
    assert!(!p.is_running());
}
