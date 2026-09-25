//! Group Distributor
//!
//! Sends a message group's items one at a time, in order, as Go's
//! `GroupDistributor` (`flowcatalyst-go/internal/outbox/group_distributor.go`):
//!
//! - each group has an in-memory FIFO of its claimed items, drained by one
//!   task, so a group never has two items in flight;
//! - at most `max_concurrent_groups` groups drain at once (0 = unbounded);
//! - with block-on-error, the first item that fails stops the group: every
//!   item still queued for it — and any submitted while it stops — is
//!   released back to PENDING, so the next poll re-claims them in order behind
//!   the failed one instead of delivering them ahead of it.
//!
//! Nothing here is the source of truth: every queued item is a row claimed
//! IN_PROGRESS in the database, so a restart loses nothing (the rows are
//! recovered to PENDING).

use async_trait::async_trait;
use fc_common::OutboxItem;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;

/// What the distributor does with a group's items.
#[async_trait]
pub trait GroupHandler: Send + Sync + 'static {
    /// Sends one item and records its outcome. `true` lets the group continue.
    async fn dispatch(&self, item: OutboxItem) -> bool;
    /// Returns items the group stopped before sending to PENDING.
    async fn release(&self, items: Vec<OutboxItem>);
}

#[derive(Default)]
struct GroupQueue {
    pending: VecDeque<OutboxItem>,
}

/// Statistics for the distributor
#[derive(Debug, Clone, Default)]
pub struct DistributorStats {
    /// Groups with a drain in progress.
    pub active_groups: usize,
    /// Items queued behind the item being sent.
    pub queued_items: usize,
}

/// Routes grouped items to per-group serial drains.
pub struct GroupDistributor {
    groups: Arc<Mutex<HashMap<String, GroupQueue>>>,
    semaphore: Option<Arc<Semaphore>>,
    block_on_error: bool,
}

impl GroupDistributor {
    /// `max_concurrent_groups` of 0 leaves group concurrency unbounded.
    pub fn new(max_concurrent_groups: usize, block_on_error: bool) -> Self {
        Self {
            groups: Arc::default(),
            semaphore: (max_concurrent_groups > 0)
                .then(|| Arc::new(Semaphore::new(max_concurrent_groups))),
            block_on_error,
        }
    }

    /// Queues `item` behind its group's earlier items, starting the group's
    /// drain if none is running. Returns at once.
    pub fn submit(&self, group: &str, item: OutboxItem, handler: Arc<dyn GroupHandler>) {
        let start = {
            let mut groups = self.groups.lock().unwrap_or_else(|e| e.into_inner());
            let fresh = !groups.contains_key(group);
            groups
                .entry(group.to_string())
                .or_default()
                .pending
                .push_back(item);
            fresh
        };
        if start {
            let groups = Arc::clone(&self.groups);
            let semaphore = self.semaphore.clone();
            let group = group.to_string();
            let block_on_error = self.block_on_error;
            tokio::spawn(async move {
                drain(groups, group, semaphore, block_on_error, handler).await;
            });
        }
    }

    pub fn stats(&self) -> DistributorStats {
        let groups = self.groups.lock().unwrap_or_else(|e| e.into_inner());
        DistributorStats {
            active_groups: groups.len(),
            queued_items: groups.values().map(|q| q.pending.len()).sum(),
        }
    }
}

async fn drain(
    groups: Arc<Mutex<HashMap<String, GroupQueue>>>,
    group: String,
    semaphore: Option<Arc<Semaphore>>,
    block_on_error: bool,
    handler: Arc<dyn GroupHandler>,
) {
    // Bounded group concurrency: the drain waits here, its items stay queued.
    let _permit = match semaphore {
        Some(s) => s.acquire_owned().await.ok(),
        None => None,
    };

    let mut stopped = false;
    loop {
        if stopped {
            // Release everything queued, including items submitted while the
            // group stops, then drop the group. The entry stays until the
            // release is done, so nothing new starts a drain ahead of it.
            let items: Vec<OutboxItem> = {
                let mut guard = groups.lock().unwrap_or_else(|e| e.into_inner());
                match guard.get_mut(&group) {
                    Some(q) if !q.pending.is_empty() => q.pending.drain(..).collect(),
                    _ => {
                        guard.remove(&group);
                        return;
                    }
                }
            };
            handler.release(items).await;
            continue;
        }

        let item = {
            let mut guard = groups.lock().unwrap_or_else(|e| e.into_inner());
            match guard.get_mut(&group).and_then(|q| q.pending.pop_front()) {
                Some(item) => item,
                None => {
                    // Drained: drop the entry so the map doesn't grow with
                    // every group ever seen. A later submit re-creates it.
                    guard.remove(&group);
                    return;
                }
            }
        };

        if !handler.dispatch(item).await && block_on_error {
            stopped = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use fc_common::{OutboxItemType, OutboxStatus};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    fn item(id: &str, group: &str) -> OutboxItem {
        OutboxItem {
            id: id.to_string(),
            item_type: OutboxItemType::Event,
            message_group: Some(group.to_string()),
            payload: serde_json::json!({}),
            status: OutboxStatus::InProgress,
            retry_count: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            error_message: None,
            client_id: None,
            payload_size: None,
            headers: None,
        }
    }

    #[derive(Default)]
    struct Recorder {
        fail: Vec<String>,
        sent: Mutex<Vec<String>>,
        released: Mutex<Vec<String>>,
        running: AtomicUsize,
        max_running: AtomicUsize,
        delay: Duration,
    }

    #[async_trait]
    impl GroupHandler for Recorder {
        async fn dispatch(&self, item: OutboxItem) -> bool {
            let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_running.fetch_max(now, Ordering::SeqCst);
            tokio::time::sleep(self.delay).await;
            self.running.fetch_sub(1, Ordering::SeqCst);
            self.sent.lock().unwrap().push(item.id.clone());
            !self.fail.contains(&item.id)
        }
        async fn release(&self, items: Vec<OutboxItem>) {
            self.released
                .lock()
                .unwrap()
                .extend(items.into_iter().map(|i| i.id));
        }
    }

    async fn settle(d: &GroupDistributor) {
        for _ in 0..200 {
            if d.stats().active_groups == 0 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("distributor did not drain");
    }

    #[tokio::test]
    async fn a_group_is_sent_in_order() {
        let d = GroupDistributor::new(10, true);
        let h = Arc::new(Recorder::default());
        for id in ["a", "b", "c"] {
            d.submit("g", item(id, "g"), h.clone());
        }
        settle(&d).await;
        assert_eq!(*h.sent.lock().unwrap(), vec!["a", "b", "c"]);
    }

    #[tokio::test]
    async fn a_failure_stops_the_group_and_releases_the_rest() {
        let d = GroupDistributor::new(10, true);
        let h = Arc::new(Recorder {
            fail: vec!["b".into()],
            ..Default::default()
        });
        for id in ["a", "b", "c", "d"] {
            d.submit("g", item(id, "g"), h.clone());
        }
        d.submit("other", item("x", "other"), h.clone());
        settle(&d).await;
        let mut sent = h.sent.lock().unwrap().clone();
        sent.sort();
        assert_eq!(sent, vec!["a", "b", "x"]);
        assert_eq!(*h.released.lock().unwrap(), vec!["c", "d"]);
    }

    #[tokio::test]
    async fn without_block_on_error_a_failure_does_not_stop_the_group() {
        let d = GroupDistributor::new(10, false);
        let h = Arc::new(Recorder {
            fail: vec!["a".into()],
            ..Default::default()
        });
        for id in ["a", "b"] {
            d.submit("g", item(id, "g"), h.clone());
        }
        settle(&d).await;
        assert_eq!(*h.sent.lock().unwrap(), vec!["a", "b"]);
        assert!(h.released.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn group_concurrency_is_bounded() {
        let d = GroupDistributor::new(2, true);
        let h = Arc::new(Recorder {
            delay: Duration::from_millis(20),
            ..Default::default()
        });
        for g in 0..6 {
            let group = format!("g{g}");
            d.submit(&group, item(&format!("i{g}"), &group), h.clone());
        }
        settle(&d).await;
        assert_eq!(h.sent.lock().unwrap().len(), 6);
        assert_eq!(h.max_running.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn items_submitted_while_a_group_stops_are_released_too() {
        let d = Arc::new(GroupDistributor::new(10, true));
        let h = Arc::new(Recorder {
            fail: vec!["a".into()],
            delay: Duration::from_millis(30),
            ..Default::default()
        });
        d.submit("g", item("a", "g"), h.clone());
        // Arrives while "a" is being sent; "a" then fails.
        tokio::time::sleep(Duration::from_millis(10)).await;
        d.submit("g", item("b", "g"), h.clone());
        settle(&d).await;
        assert_eq!(*h.sent.lock().unwrap(), vec!["a"]);
        assert_eq!(*h.released.lock().unwrap(), vec!["b"]);
    }
}
