//! Fakes for the scheduler's unit tests: an in-memory job table that behaves
//! like the claim and the QUEUED update, and a publisher that records.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::{Notify, Semaphore};
use tokio::time;

use super::auth::DispatchAuthService;
use super::dispatcher::{DispatchJobToken, MessageGroupDispatcher};
use super::lane::{ClaimedJob, LaneJob};
use super::poller::{JobStore, PendingJobPoller, PollerSettings};
use super::publisher::{DispatchPublisher, PublishItem, PublishOutcome};
use super::SchedulerError;

pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Pending,
    Queued,
}

struct Row {
    job: ClaimedJob,
    status: Status,
}

/// (call number, entered, proceed): see [`FakeStore::gate_claim`].
type ClaimGate = (usize, Arc<Notify>, Arc<Semaphore>);

/// One claim the fake saw.
#[derive(Debug, Clone)]
pub(crate) struct ClaimLog {
    pub exclude: Vec<String>,
    pub returned: Vec<String>,
}

/// A job table in claim order. `claim` returns the first `limit` PENDING rows
/// whose id is not excluded, like the real query; `mark_queued` moves only
/// PENDING rows.
#[derive(Default)]
pub(crate) struct FakeStore {
    rows: Mutex<Vec<Row>>,
    pub claims: Mutex<Vec<ClaimLog>>,
    pub fail_claim: AtomicBool,
    pub fail_mark: AtomicBool,
    pub claim_calls: AtomicUsize,
    /// When set, the claim with this call number (1-based) announces on
    /// `entered` once it has its arguments (generation and snapshot already
    /// taken) and waits for a permit on `proceed` before it reads the table.
    gate: Mutex<Option<ClaimGate>>,
}

pub(crate) fn token(id: &str, group: Option<&str>) -> DispatchJobToken {
    DispatchJobToken {
        job_id: id.to_string(),
        message_group: group.map(str::to_string),
        mode: "IMMEDIATE".into(),
        pool_code: "P".into(),
        client_id: None,
        subscription_id: None,
        queue: None,
    }
}

pub(crate) fn claimed(id: &str, group: Option<&str>) -> ClaimedJob {
    ClaimedJob {
        token: token(id, group),
        created_at: DateTime::<Utc>::UNIX_EPOCH,
    }
}

pub(crate) fn lane_job(id: &str, group: Option<&str>, generation: u64) -> LaneJob {
    LaneJob {
        job: claimed(id, group),
        generation,
    }
}

impl FakeStore {
    pub fn with_jobs(jobs: Vec<(String, Option<String>)>) -> Arc<Self> {
        let store = Self::default();
        for (id, group) in jobs {
            lock(&store.rows).push(Row {
                job: claimed(&id, group.as_deref()),
                status: Status::Pending,
            });
        }
        Arc::new(store)
    }

    pub fn status_of(&self, id: &str) -> Status {
        lock(&self.rows)
            .iter()
            .find(|r| r.job.id() == id)
            .map(|r| r.status)
            .unwrap()
    }

    pub fn count(&self, status: Status) -> usize {
        lock(&self.rows)
            .iter()
            .filter(|r| r.status == status)
            .count()
    }

    pub fn ids_with(&self, status: Status) -> Vec<String> {
        lock(&self.rows)
            .iter()
            .filter(|r| r.status == status)
            .map(|r| r.job.id().to_string())
            .collect()
    }

    /// Gate the `call`-th claim (1-based); returns (entered, proceed).
    pub fn gate_claim(&self, call: usize) -> (Arc<Notify>, Arc<Semaphore>) {
        let (entered, proceed) = (Arc::new(Notify::new()), Arc::new(Semaphore::new(0)));
        *lock(&self.gate) = Some((call, entered.clone(), proceed.clone()));
        (entered, proceed)
    }
}

#[async_trait]
impl JobStore for FakeStore {
    async fn claim(
        &self,
        limit: usize,
        exclude: Vec<String>,
    ) -> Result<Vec<ClaimedJob>, SchedulerError> {
        let call = self.claim_calls.fetch_add(1, Ordering::SeqCst) + 1;
        // A poller that claims in a hot loop never lets paused time advance,
        // so a test would hang instead of failing.
        assert!(call <= 2000, "the poller is claiming in a hot loop");
        let gate = {
            let mut g = lock(&self.gate);
            if g.as_ref().is_some_and(|(n, _, _)| *n == call) {
                g.take()
            } else {
                None
            }
        };
        if let Some((_, entered, proceed)) = gate {
            entered.notify_one();
            proceed.acquire().await.unwrap().forget();
        }
        if self.fail_claim.load(Ordering::SeqCst) {
            return Err(SchedulerError::ConfigError("claim failed".into()));
        }
        let excluded: HashSet<&str> = exclude.iter().map(String::as_str).collect();
        let returned: Vec<ClaimedJob> = lock(&self.rows)
            .iter()
            .filter(|r| r.status == Status::Pending && !excluded.contains(r.job.id()))
            .take(limit)
            .map(|r| r.job.clone())
            .collect();
        lock(&self.claims).push(ClaimLog {
            exclude,
            returned: returned.iter().map(|j| j.id().to_string()).collect(),
        });
        Ok(returned)
    }

    async fn mark_queued(&self, jobs: &[(String, DateTime<Utc>)]) -> Result<u64, SchedulerError> {
        if self.fail_mark.load(Ordering::SeqCst) {
            return Err(SchedulerError::ConfigError("mark failed".into()));
        }
        let ids: HashSet<&str> = jobs.iter().map(|(id, _)| id.as_str()).collect();
        let mut n = 0;
        for r in lock(&self.rows).iter_mut() {
            if ids.contains(r.job.id()) && r.status == Status::Pending {
                r.status = Status::Queued;
                n += 1;
            }
        }
        Ok(n)
    }
}

/// Records what it publishes, in order. Behaves like the real publishers on
/// failure: a failed job and every later job of its group in the call are
/// unpublished.
#[derive(Default)]
pub(crate) struct FakePublisher {
    pub published: Mutex<Vec<String>>,
    /// Ids that fail while present.
    pub fail_ids: Mutex<HashSet<String>>,
    /// Ids that fail the first time they are published.
    pub fail_once: Mutex<HashSet<String>>,
    pub fail_all: AtomicBool,
    pub calls: AtomicUsize,
    /// Each call first takes a permit from here when set.
    pub gate: Mutex<Option<Arc<Semaphore>>>,
    /// Virtual time each call takes.
    pub delay: Mutex<Duration>,
    /// Announced when a call starts.
    pub started: Notify,
}

impl FakePublisher {
    pub fn published_ids(&self) -> Vec<String> {
        lock(&self.published).clone()
    }

    /// Per group, the ids published, in order. `group_of` maps an id to its
    /// group.
    pub fn per_group(&self, group_of: impl Fn(&str) -> String) -> HashMap<String, Vec<String>> {
        let mut out: HashMap<String, Vec<String>> = HashMap::new();
        for id in lock(&self.published).iter() {
            out.entry(group_of(id)).or_default().push(id.clone());
        }
        out
    }
}

#[async_trait]
impl DispatchPublisher for FakePublisher {
    async fn publish(&self, items: Vec<PublishItem>) -> PublishOutcome {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.started.notify_one();
        let gate = lock(&self.gate).clone();
        if let Some(gate) = gate {
            gate.acquire().await.unwrap().forget();
        }
        let delay = *lock(&self.delay);
        if !delay.is_zero() {
            time::sleep(delay).await;
        }
        if self.fail_all.load(Ordering::SeqCst) {
            return PublishOutcome {
                unpublished: items.iter().map(|i| i.job_id.clone()).collect(),
                error: Some("broker down".into()),
            };
        }
        let mut failed_groups: HashSet<String> = HashSet::new();
        let mut unpublished = Vec::new();
        for item in &items {
            let group = item.group_id().to_string();
            let once = lock(&self.fail_once).remove(&item.job_id);
            if failed_groups.contains(&group) || once || lock(&self.fail_ids).contains(&item.job_id)
            {
                failed_groups.insert(group);
                unpublished.push(item.job_id.clone());
            } else {
                lock(&self.published).push(item.job_id.clone());
            }
        }
        PublishOutcome {
            error: (!unpublished.is_empty()).then(|| "failed".to_string()),
            unpublished,
        }
    }

    fn describe(&self) -> String {
        "fake".into()
    }
}

pub(crate) fn dispatcher(publisher: Arc<FakePublisher>) -> Arc<MessageGroupDispatcher> {
    Arc::new(MessageGroupDispatcher::new(
        publisher,
        DispatchAuthService::with_secret("s"),
        "http://x/process".into(),
    ))
}

pub(crate) fn settings(
    buffer_capacity: usize,
    dispatchers: usize,
    batch_size: usize,
) -> PollerSettings {
    PollerSettings {
        buffer_capacity,
        dispatchers,
        batch_size,
        lane_batch: 100,
    }
}

pub(crate) struct Rig {
    pub store: Arc<FakeStore>,
    pub publisher: Arc<FakePublisher>,
    pub poller: PendingJobPoller,
}

pub(crate) fn rig(store: Arc<FakeStore>, settings: PollerSettings) -> Rig {
    let publisher = Arc::new(FakePublisher::default());
    let poller =
        PendingJobPoller::with_store(store.clone(), dispatcher(publisher.clone()), settings);
    Rig {
        store,
        publisher,
        poller,
    }
}
