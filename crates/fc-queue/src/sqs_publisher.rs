//! SQS FIFO batch publisher for the dispatch scheduler.
//!
//! A port of Go's `SQSDispatchPublisher`
//! (`flowcatalyst-go/internal/platform/scheduler/publisher_sqs.go`). The
//! scheduler claims up to a batch of jobs per tick, resolves each one's
//! destination queue, and hands the whole claim-ordered batch here. This
//! publisher knows nothing about jobs beyond their id, their destination and
//! their FIFO group: the body is rendered by the caller.
//!
//! # Chunking and per-queue grouping
//!
//! SQS caps `SendMessageBatch` at 10 entries and one call addresses one queue.
//! [`SqsFifoPublisher::publish`] partitions the batch into one ordered list per
//! destination (first-seen order) and sends each queue's chunks sequentially.
//! Two items bound for different queues have no order to preserve.
//!
//! Within a queue the claim arrives sorted by group, so most of a group's
//! items are adjacent. Chunks are therefore built round-robin across groups
//! ([`ChunkPlanner`]): each chunk takes the next unsent item of up to ten
//! different groups (groups in order of first appearance, the rotation
//! continuing where the previous chunk stopped), so ten groups of ten items
//! cost ten full calls rather than ninety-one nearly empty ones. A group's
//! items still go out in their original order, one per chunk.
//!
//! # No two items of one group ever share a chunk
//!
//! SQS reports batch failures per entry. If the earlier of two same-group
//! entries failed and the later succeeded, the later would be durably queued
//! while the earlier reverts and is published again afterwards: the group
//! would be delivered out of order. Two rules prevent it:
//!
//! 1. A chunk takes at most one item per group, and a group's next item is
//!    only ever in a later chunk, sent after the earlier chunk has returned.
//! 2. A group that fails poisons its own later items for the rest of the call:
//!    they are reported unpublished without being sent.
//!
//! # FIFO identifiers
//!
//! `MessageGroupId` is the item's group (the caller passes the job id for a
//! group-less job). `MessageDeduplicationId` is `{id}:{nonce}` with a nonce
//! fresh for every [`SqsFifoPublisher::publish`] call, so a genuine re-publish
//! (stale recovery, a retry after backoff) is never swallowed by the broker's
//! five-minute deduplication window.
//!
//! # Lazily created queues
//!
//! A send to a queue that does not exist creates it (FIFO, content-based
//! deduplication off) and retries the same request once.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Duration;

use async_trait::async_trait;
use aws_sdk_sqs::config::timeout::TimeoutConfig;
use aws_sdk_sqs::error::DisplayErrorContext;
use aws_sdk_sqs::types::{QueueAttributeName, SendMessageBatchRequestEntry};
use aws_sdk_sqs::Client;
use tracing::{info, warn};

/// SQS's hard cap on one `SendMessageBatch`.
pub const MAX_SQS_BATCH_SIZE: usize = 10;

/// The most one dispatch publish call (including the SDK's own retries) may
/// take before it is abandoned. A hung broker call must not hold the
/// scheduler's claim transaction, and its row locks, open indefinitely.
pub const SQS_PUBLISH_OPERATION_TIMEOUT: Duration = Duration::from_secs(25);

/// The most one HTTP attempt of a publish call may take; a slow attempt is
/// retried within [`SQS_PUBLISH_OPERATION_TIMEOUT`].
pub const SQS_PUBLISH_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(10);

/// SQS's hard cap on a `MessageDeduplicationId`.
pub const MAX_DEDUP_ID_LENGTH: usize = 128;

/// One message on its way to a named FIFO queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FifoPublishItem {
    /// The caller's id for the item (the dispatch job id). Also the batch
    /// entry id, so it must be unique within one publish call.
    pub id: String,
    /// The destination queue's name (e.g. `FC-prod-acme-DEFAULT.fifo`).
    pub queue_name: String,
    /// The FIFO `MessageGroupId`.
    pub group_id: String,
    /// The message body, already rendered.
    pub body: String,
}

/// One entry of a `SendMessageBatch`, as the API receives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FifoEntry {
    pub id: String,
    pub body: String,
    pub group_id: String,
    pub dedup_id: String,
}

/// Why a batch send failed as a whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendBatchError {
    /// The queue does not exist (yet).
    QueueMissing,
    /// Anything else; the text is for logging.
    Other(String),
}

/// The result of one [`SqsFifoPublisher::publish`] call.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FifoPublishOutcome {
    /// The ids NOT published. The caller reverts exactly these: an id the
    /// broker accepted is never listed, and one it did not accept always is.
    pub unpublished: Vec<String>,
    /// The last error seen, for logging only; carries no ids.
    pub error: Option<String>,
}

/// The slice of the SQS API the publisher uses, so its rules can be tested
/// without a broker.
#[async_trait]
pub trait SqsBatchApi: Send + Sync {
    /// Send one batch. `Ok` carries the entry ids SQS reported failed.
    async fn send_batch(
        &self,
        queue_url: &str,
        entries: &[FifoEntry],
    ) -> Result<HashSet<String>, SendBatchError>;

    /// Create a FIFO queue with content-based deduplication off. A queue
    /// that already exists is success.
    async fn create_fifo_queue(&self, queue_name: &str) -> Result<(), String>;
}

/// How a queue name becomes a queue URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueAddressing {
    /// `https://sqs.{region}.amazonaws.com/{account_id}/{name}`, as Go's
    /// `Settings.QueueURIFor` composes it.
    Composed { region: String, account_id: String },
    /// `{base}/{name}`: an emulator's own URL shape.
    Base(String),
}

impl QueueAddressing {
    pub fn url_for(&self, queue_name: &str) -> String {
        match self {
            QueueAddressing::Composed { region, account_id } => {
                format!("https://sqs.{region}.amazonaws.com/{account_id}/{queue_name}")
            }
            QueueAddressing::Base(base) => {
                format!("{}/{}", base.trim_end_matches('/'), queue_name)
            }
        }
    }
}

/// The AWS SDK implementation of [`SqsBatchApi`].
#[derive(Clone)]
pub struct AwsSqsBatchApi {
    client: Client,
}

impl AwsSqsBatchApi {
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

/// Error codes SQS (and its emulators, which answer in the query-compatible
/// dialect) use for "that queue does not exist".
fn is_queue_missing_code(code: Option<&str>) -> bool {
    matches!(
        code,
        Some("AWS.SimpleQueueService.NonExistentQueue") | Some("QueueDoesNotExist")
    )
}

#[async_trait]
impl SqsBatchApi for AwsSqsBatchApi {
    async fn send_batch(
        &self,
        queue_url: &str,
        entries: &[FifoEntry],
    ) -> Result<HashSet<String>, SendBatchError> {
        let mut request = self.client.send_message_batch().queue_url(queue_url);
        for e in entries {
            let entry = SendMessageBatchRequestEntry::builder()
                .id(&e.id)
                .message_body(&e.body)
                .message_group_id(&e.group_id)
                .message_deduplication_id(&e.dedup_id)
                .build()
                .map_err(|err| SendBatchError::Other(err.to_string()))?;
            request = request.entries(entry);
        }
        match request.send().await {
            Ok(out) => Ok(out.failed().iter().map(|f| f.id().to_string()).collect()),
            Err(err) => {
                use aws_sdk_sqs::error::ProvideErrorMetadata;
                let missing = err
                    .as_service_error()
                    .map(|s| s.is_queue_does_not_exist() || is_queue_missing_code(s.code()))
                    .unwrap_or(false);
                if missing {
                    Err(SendBatchError::QueueMissing)
                } else {
                    Err(SendBatchError::Other(DisplayErrorContext(&err).to_string()))
                }
            }
        }
    }

    async fn create_fifo_queue(&self, queue_name: &str) -> Result<(), String> {
        let result = self
            .client
            .create_queue()
            .queue_name(queue_name)
            .attributes(QueueAttributeName::FifoQueue, "true")
            .attributes(QueueAttributeName::ContentBasedDeduplication, "false")
            .send()
            .await;
        match result {
            Ok(_) => {
                info!(queue = %queue_name, "created dispatch queue on first publish");
                Ok(())
            }
            Err(err) => {
                // Another instance (or another chunk of this same call)
                // created it between the failed send and here.
                if err
                    .as_service_error()
                    .map(|s| s.is_queue_name_exists())
                    .unwrap_or(false)
                {
                    return Ok(());
                }
                Err(format!(
                    "create dispatch queue {queue_name:?}: {}",
                    DisplayErrorContext(&err)
                ))
            }
        }
    }
}

/// Publishes claim-ordered items to their FIFO queues. See the module docs.
pub struct SqsFifoPublisher<A: SqsBatchApi = AwsSqsBatchApi> {
    api: A,
    addressing: QueueAddressing,
}

impl SqsFifoPublisher<AwsSqsBatchApi> {
    /// A publisher over the SDK's default credential chain, in `region` when
    /// given (else the chain's own region). `AWS_ENDPOINT_URL[_SQS]` is
    /// honoured by the SDK, as by Go's.
    pub async fn from_default_chain(region: Option<String>, addressing: QueueAddressing) -> Self {
        let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest());
        if let Some(region) = region.filter(|r| !r.trim().is_empty()) {
            loader = loader.region(aws_config::Region::new(region));
        }
        let config = loader.load().await;
        let client = with_publish_timeouts(&Client::new(&config));
        Self::new(AwsSqsBatchApi::new(client), addressing)
    }
}

/// `client` with the publish operation and attempt timeouts built in, once.
/// The SDK sets none by default, so a hung call would wait forever. Other
/// timeouts the client already carries (the default chain's connect
/// timeout) are kept.
fn with_publish_timeouts(client: &Client) -> Client {
    let base = match client.config().timeout_config() {
        Some(existing) => existing.clone().to_builder(),
        None => TimeoutConfig::builder(),
    };
    let timeouts = base
        .operation_timeout(SQS_PUBLISH_OPERATION_TIMEOUT)
        .operation_attempt_timeout(SQS_PUBLISH_ATTEMPT_TIMEOUT)
        .build();
    Client::from_conf(
        client
            .config()
            .to_builder()
            .timeout_config(timeouts)
            .build(),
    )
}

impl<A: SqsBatchApi> SqsFifoPublisher<A> {
    pub fn new(api: A, addressing: QueueAddressing) -> Self {
        Self { api, addressing }
    }

    pub fn addressing(&self) -> &QueueAddressing {
        &self.addressing
    }

    /// Send every item to its queue, returning the ids not published.
    pub async fn publish(&self, items: Vec<FifoPublishItem>) -> FifoPublishOutcome {
        if items.is_empty() {
            return FifoPublishOutcome::default();
        }
        let total = items.len();

        // Partition by destination, first-seen order, claim order within.
        let mut order: Vec<String> = Vec::new();
        let mut by_queue: HashMap<String, Vec<FifoPublishItem>> = HashMap::new();
        for item in items {
            if !by_queue.contains_key(&item.queue_name) {
                order.push(item.queue_name.clone());
            }
            by_queue
                .entry(item.queue_name.clone())
                .or_default()
                .push(item);
        }

        // One nonce per call, shared by every chunk.
        let nonce = uuid::Uuid::new_v4().to_string();
        let mut failed_groups: HashSet<String> = HashSet::new();
        let mut unpublished: Vec<String> = Vec::new();
        let mut last_error: Option<String> = None;

        for queue_name in &order {
            let queued = by_queue.remove(queue_name).unwrap_or_default();
            let mut planner = ChunkPlanner::new(queued);
            loop {
                let chunk = planner.next_chunk(&failed_groups, &mut unpublished);
                if chunk.is_empty() {
                    break;
                }
                match self.send_chunk(queue_name, &chunk, &nonce).await {
                    Ok(failed_ids) => {
                        for item in &chunk {
                            if failed_ids.contains(&item.id) {
                                unpublished.push(item.id.clone());
                                failed_groups.insert(item.group_id.clone());
                            }
                        }
                        if !failed_ids.is_empty() {
                            last_error = Some(format!(
                                "SQS rejected {} entr(ies) sent to {queue_name}",
                                failed_ids.len()
                            ));
                        }
                    }
                    Err(err) => {
                        // Every other chunk and queue is still attempted: one
                        // client's queue must not strand the rest of the batch.
                        warn!(queue = %queue_name, count = chunk.len(), error = %err,
                            "sqs dispatch chunk failed; job(s) will revert to PENDING");
                        for item in &chunk {
                            unpublished.push(item.id.clone());
                            failed_groups.insert(item.group_id.clone());
                        }
                        last_error = Some(err);
                    }
                }
            }
        }

        if unpublished.is_empty() {
            return FifoPublishOutcome::default();
        }
        let error = Some(format!(
            "sqs dispatch publish failed for {} of {total} message(s): {}",
            unpublished.len(),
            last_error.unwrap_or_else(|| "earlier failure in the same group".to_string())
        ));
        FifoPublishOutcome { unpublished, error }
    }

    /// Send one chunk, creating the queue and retrying once when it is
    /// missing. Returns the ids SQS itself reported failed.
    async fn send_chunk(
        &self,
        queue_name: &str,
        chunk: &[FifoPublishItem],
        nonce: &str,
    ) -> Result<HashSet<String>, String> {
        let url = self.addressing.url_for(queue_name);
        let entries: Vec<FifoEntry> = chunk
            .iter()
            .map(|item| FifoEntry {
                id: item.id.clone(),
                body: item.body.clone(),
                group_id: item.group_id.clone(),
                dedup_id: dedup_id(&item.id, nonce),
            })
            .collect();
        match self.api.send_batch(&url, &entries).await {
            Ok(failed) => Ok(failed),
            Err(SendBatchError::Other(e)) => Err(e),
            Err(SendBatchError::QueueMissing) => {
                self.api.create_fifo_queue(queue_name).await?;
                // The same entries and dedup ids: the first attempt never
                // reached a queue that could have deduplicated anything.
                match self.api.send_batch(&url, &entries).await {
                    Ok(failed) => Ok(failed),
                    Err(SendBatchError::QueueMissing) => {
                        Err(format!("queue {queue_name} still missing after create"))
                    }
                    Err(SendBatchError::Other(e)) => Err(e),
                }
            }
        }
    }
}

/// One group's unsent items, in claim order.
struct GroupQueue {
    group_id: String,
    items: VecDeque<FifoPublishItem>,
}

/// Builds one destination queue's chunks round-robin across its groups.
///
/// The groups sit in a ring in order of first appearance. A chunk takes the
/// front item of each of the next (up to ten) groups, so it never holds two
/// items of one group; the groups it took go to the back of the ring, so the
/// next chunk continues where this one stopped and full chunks stay full
/// while at least ten groups have items left. Chunks are produced lazily so
/// that a failure reported after one chunk poisons the group's remaining
/// items before they are packed.
struct ChunkPlanner {
    ring: VecDeque<GroupQueue>,
}

impl ChunkPlanner {
    fn new(queued: Vec<FifoPublishItem>) -> Self {
        let mut index: HashMap<String, usize> = HashMap::new();
        let mut groups: Vec<GroupQueue> = Vec::new();
        for item in queued {
            let i = *index.entry(item.group_id.clone()).or_insert_with(|| {
                groups.push(GroupQueue {
                    group_id: item.group_id.clone(),
                    items: VecDeque::new(),
                });
                groups.len() - 1
            });
            groups[i].items.push_back(item);
        }
        Self {
            ring: groups.into(),
        }
    }

    /// The next chunk, empty when nothing is left. Items of a group in
    /// `failed_groups` are moved to `unpublished` without being sent.
    fn next_chunk(
        &mut self,
        failed_groups: &HashSet<String>,
        unpublished: &mut Vec<String>,
    ) -> Vec<FifoPublishItem> {
        self.ring.retain(|g| {
            if failed_groups.contains(&g.group_id) {
                unpublished.extend(g.items.iter().map(|i| i.id.clone()));
                false
            } else {
                true
            }
        });
        let mut chunk = Vec::with_capacity(MAX_SQS_BATCH_SIZE);
        let mut taken: Vec<GroupQueue> = Vec::new();
        while chunk.len() < MAX_SQS_BATCH_SIZE {
            let Some(mut g) = self.ring.pop_front() else {
                break;
            };
            if let Some(item) = g.items.pop_front() {
                chunk.push(item);
            }
            if !g.items.is_empty() {
                taken.push(g);
            }
        }
        self.ring.extend(taken);
        chunk
    }
}

/// `{id}:{nonce}`, clipped to SQS's limit. Never the bare id: see the module
/// docs for why a per-call nonce is the point.
pub fn dedup_id(id: &str, nonce: &str) -> String {
    let mut out = format!("{id}:{nonce}");
    if out.len() > MAX_DEDUP_ID_LENGTH {
        let mut cut = MAX_DEDUP_ID_LENGTH;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeApi {
        sends: Mutex<Vec<(String, Vec<FifoEntry>)>>,
        created: Mutex<Vec<String>>,
        /// Queue URLs that fail the whole send with this error.
        fail_url: Mutex<HashMap<String, SendBatchError>>,
        /// Entry ids SQS reports failed.
        reject_ids: Mutex<HashSet<String>>,
        /// Queue URLs missing until created.
        missing: Mutex<HashSet<String>>,
    }

    #[async_trait]
    impl SqsBatchApi for FakeApi {
        async fn send_batch(
            &self,
            queue_url: &str,
            entries: &[FifoEntry],
        ) -> Result<HashSet<String>, SendBatchError> {
            if self.missing.lock().unwrap().contains(queue_url) {
                return Err(SendBatchError::QueueMissing);
            }
            if let Some(e) = self.fail_url.lock().unwrap().get(queue_url) {
                return Err(e.clone());
            }
            self.sends
                .lock()
                .unwrap()
                .push((queue_url.to_string(), entries.to_vec()));
            let rejects = self.reject_ids.lock().unwrap();
            Ok(entries
                .iter()
                .filter(|e| rejects.contains(&e.id))
                .map(|e| e.id.clone())
                .collect())
        }

        async fn create_fifo_queue(&self, queue_name: &str) -> Result<(), String> {
            self.created.lock().unwrap().push(queue_name.to_string());
            let url = QueueAddressing::Base("http://sqs".into()).url_for(queue_name);
            self.missing.lock().unwrap().remove(&url);
            Ok(())
        }
    }

    fn item(id: &str, queue: &str, group: &str) -> FifoPublishItem {
        FifoPublishItem {
            id: id.to_string(),
            queue_name: queue.to_string(),
            group_id: group.to_string(),
            body: format!("{{\"id\":\"{id}\"}}"),
        }
    }

    fn publisher() -> SqsFifoPublisher<FakeApi> {
        SqsFifoPublisher::new(
            FakeApi::default(),
            QueueAddressing::Base("http://sqs".into()),
        )
    }

    fn sent_ids(p: &SqsFifoPublisher<FakeApi>) -> Vec<Vec<String>> {
        p.api
            .sends
            .lock()
            .unwrap()
            .iter()
            .map(|(_, es)| es.iter().map(|e| e.id.clone()).collect())
            .collect()
    }

    /// The publisher's client carries both timeouts (and keeps what the
    /// client already had).
    #[test]
    fn publish_client_carries_operation_and_attempt_timeouts() {
        use aws_sdk_sqs::config::{BehaviorVersion, Region};
        let connect = Duration::from_millis(3100);
        let conf = aws_sdk_sqs::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new("us-east-1"))
            .timeout_config(TimeoutConfig::builder().connect_timeout(connect).build())
            .build();
        let client = with_publish_timeouts(&Client::from_conf(conf));
        let t = client.config().timeout_config().cloned().unwrap();
        assert_eq!(t.operation_timeout(), Some(SQS_PUBLISH_OPERATION_TIMEOUT));
        assert_eq!(
            t.operation_attempt_timeout(),
            Some(SQS_PUBLISH_ATTEMPT_TIMEOUT)
        );
        assert_eq!(t.connect_timeout(), Some(connect));
        assert!(SQS_PUBLISH_ATTEMPT_TIMEOUT < SQS_PUBLISH_OPERATION_TIMEOUT);
    }

    #[test]
    fn composed_addressing_matches_go() {
        let a = QueueAddressing::Composed {
            region: "eu-west-1".into(),
            account_id: "123456789012".into(),
        };
        assert_eq!(
            a.url_for("FC-staging-acme-DEFAULT.fifo"),
            "https://sqs.eu-west-1.amazonaws.com/123456789012/FC-staging-acme-DEFAULT.fifo"
        );
    }

    #[tokio::test]
    async fn chunks_at_ten_and_partitions_by_queue() {
        let p = publisher();
        let mut items: Vec<FifoPublishItem> = (0..23)
            .map(|i| item(&format!("a{i:02}"), "qa", &format!("a{i:02}")))
            .collect();
        items.push(item("b0", "qb", "b0"));
        let out = p.publish(items).await;
        assert!(out.unpublished.is_empty(), "{out:?}");
        let sends = p.api.sends.lock().unwrap().clone();
        let sizes: Vec<(String, usize)> = sends.iter().map(|(u, e)| (u.clone(), e.len())).collect();
        assert_eq!(
            sizes,
            vec![
                ("http://sqs/qa".to_string(), 10),
                ("http://sqs/qa".to_string(), 10),
                ("http://sqs/qa".to_string(), 3),
                ("http://sqs/qb".to_string(), 1),
            ]
        );
        // Claim order preserved within the queue.
        let flat: Vec<String> = sends[..3]
            .iter()
            .flat_map(|(_, es)| es.iter().map(|e| e.id.clone()))
            .collect();
        let want: Vec<String> = (0..23).map(|i| format!("a{i:02}")).collect();
        assert_eq!(flat, want);
    }

    #[tokio::test]
    async fn never_two_of_one_group_in_a_chunk() {
        let p = publisher();
        let items = vec![
            item("1", "q", "g"),
            item("2", "q", "g"),
            item("3", "q", "h"),
            item("4", "q", "g"),
        ];
        let out = p.publish(items).await;
        assert!(out.unpublished.is_empty());
        assert_eq!(
            sent_ids(&p),
            vec![
                vec!["1".to_string(), "3".into()],
                vec!["2".into()],
                vec!["4".into()]
            ]
        );
    }

    #[tokio::test]
    async fn fifo_fields_and_per_call_dedup_nonce() {
        let p = publisher();
        p.publish(vec![item("job1", "q", "grp")]).await;
        p.publish(vec![item("job1", "q", "grp")]).await;
        let sends = p.api.sends.lock().unwrap().clone();
        let a = &sends[0].1[0];
        let b = &sends[1].1[0];
        assert_eq!(a.group_id, "grp");
        assert!(a.dedup_id.starts_with("job1:"));
        assert_ne!(a.dedup_id, "job1", "never the bare id");
        assert_ne!(a.dedup_id, b.dedup_id, "a fresh nonce per publish call");
    }

    #[tokio::test]
    async fn creates_a_missing_queue_and_retries_once() {
        let p = publisher();
        p.api
            .missing
            .lock()
            .unwrap()
            .insert("http://sqs/new.fifo".into());
        let out = p.publish(vec![item("1", "new.fifo", "1")]).await;
        assert!(out.unpublished.is_empty());
        assert_eq!(*p.api.created.lock().unwrap(), vec!["new.fifo".to_string()]);
        assert_eq!(sent_ids(&p), vec![vec!["1".to_string()]]);
    }

    #[tokio::test]
    async fn reverts_only_the_failed_entries() {
        let p = publisher();
        p.api.reject_ids.lock().unwrap().insert("2".into());
        let out = p
            .publish(vec![
                item("1", "q", "a"),
                item("2", "q", "b"),
                item("3", "q", "c"),
            ])
            .await;
        assert_eq!(out.unpublished, vec!["2".to_string()]);
        assert!(out.error.is_some());
    }

    #[tokio::test]
    async fn a_failed_group_poisons_its_later_items() {
        let p = publisher();
        p.api.reject_ids.lock().unwrap().insert("1".into());
        let out = p
            .publish(vec![
                item("1", "q", "g"),
                item("2", "q", "g"),
                item("3", "q", "h"),
                item("4", "q", "g"),
            ])
            .await;
        // 1 rejected; 2 and 4 never sent; 3 delivered.
        let mut un = out.unpublished.clone();
        un.sort();
        assert_eq!(un, vec!["1", "2", "4"]);
        let sent: Vec<String> = sent_ids(&p).into_iter().flatten().collect();
        assert!(!sent.contains(&"2".to_string()) && !sent.contains(&"4".to_string()));
        assert!(sent.contains(&"3".to_string()));
    }

    /// Claim order: sorted by group, as the scheduler's claim query yields.
    fn grouped(groups: usize, per_group: usize) -> Vec<FifoPublishItem> {
        (0..groups)
            .flat_map(|g| {
                (0..per_group).map(move |n| item(&format!("g{g}-{n:02}"), "q", &format!("g{g}")))
            })
            .collect()
    }

    /// No chunk repeats a group.
    fn assert_no_group_repeats(p: &SqsFifoPublisher<FakeApi>) {
        for (_, es) in p.api.sends.lock().unwrap().iter() {
            let groups: HashSet<&str> = es.iter().map(|e| e.group_id.as_str()).collect();
            assert_eq!(groups.len(), es.len(), "a group twice in one chunk: {es:?}");
        }
    }

    /// Each group's items, in the order they were sent across chunks.
    fn sent_per_group(p: &SqsFifoPublisher<FakeApi>) -> HashMap<String, Vec<String>> {
        let mut out: HashMap<String, Vec<String>> = HashMap::new();
        for (_, es) in p.api.sends.lock().unwrap().iter() {
            for e in es {
                out.entry(e.group_id.clone())
                    .or_default()
                    .push(e.id.clone());
            }
        }
        out
    }

    #[tokio::test]
    async fn ten_groups_of_ten_cost_ten_full_calls() {
        let p = publisher();
        let out = p.publish(grouped(10, 10)).await;
        assert!(out.unpublished.is_empty(), "{out:?}");
        let sizes: Vec<usize> = sent_ids(&p).iter().map(Vec::len).collect();
        assert_eq!(sizes, vec![10; 10]);
        assert_no_group_repeats(&p);
        let per_group = sent_per_group(&p);
        assert_eq!(per_group.len(), 10);
        for (g, ids) in per_group {
            let want: Vec<String> = (0..10).map(|n| format!("{g}-{n:02}")).collect();
            assert_eq!(ids, want, "group {g} order");
        }
    }

    #[tokio::test]
    async fn one_group_of_25_is_25_calls_of_one_in_order() {
        let p = publisher();
        let out = p.publish(grouped(1, 25)).await;
        assert!(out.unpublished.is_empty());
        let sent = sent_ids(&p);
        assert_eq!(sent.len(), 25);
        assert!(sent.iter().all(|c| c.len() == 1));
        let flat: Vec<String> = sent.into_iter().flatten().collect();
        let want: Vec<String> = (0..25).map(|n| format!("g0-{n:02}")).collect();
        assert_eq!(flat, want);
    }

    #[tokio::test]
    async fn grouped_and_ungrouped_mix_packs_and_keeps_order() {
        // 3 groups of 5 plus 12 ungrouped jobs (each its own group): 27 items.
        let p = publisher();
        let mut items = grouped(3, 5);
        items.extend((0..12).map(|n| item(&format!("u{n:02}"), "q", &format!("u{n:02}"))));
        let total = items.len();
        let out = p.publish(items).await;
        assert!(out.unpublished.is_empty());
        assert_no_group_repeats(&p);
        let sent = sent_ids(&p);
        assert_eq!(sent.iter().map(Vec::len).sum::<usize>(), total);
        // 15 groups live at first, so the first chunks are full.
        assert_eq!(sent[0].len(), 10);
        for (g, ids) in sent_per_group(&p) {
            if g.starts_with('g') {
                let want: Vec<String> = (0..5).map(|n| format!("{g}-{n:02}")).collect();
                assert_eq!(ids, want, "group {g} order");
            }
        }
        // 27 items need at least 3 calls and the five-deep groups force at
        // least 5; adjacent-only packing needed 5 for the grouped part plus
        // 2 for the rest.
        assert!(sent.len() <= 6, "{} calls: {sent:?}", sent.len());
    }

    #[tokio::test]
    async fn a_failed_group_stays_poisoned_across_round_robin_chunks() {
        let p = publisher();
        // g0's first item is rejected; its other items (in later chunks)
        // must never be sent, and the other groups complete in order.
        p.api.reject_ids.lock().unwrap().insert("g0-00".into());
        let out = p.publish(grouped(3, 4)).await;
        let mut un = out.unpublished.clone();
        un.sort();
        assert_eq!(un, vec!["g0-00", "g0-01", "g0-02", "g0-03"]);
        let per_group = sent_per_group(&p);
        assert_eq!(
            per_group.get("g0").map(|v| v.as_slice()),
            Some(&["g0-00".to_string()][..])
        );
        for g in ["g1", "g2"] {
            let want: Vec<String> = (0..4).map(|n| format!("{g}-{n:02}")).collect();
            assert_eq!(per_group[g], want);
        }
    }

    #[tokio::test]
    async fn a_chunk_error_does_not_abandon_other_queues() {
        let p = publisher();
        p.api.fail_url.lock().unwrap().insert(
            "http://sqs/bad".into(),
            SendBatchError::Other("boom".into()),
        );
        let out = p
            .publish(vec![item("1", "bad", "1"), item("2", "good", "2")])
            .await;
        assert_eq!(out.unpublished, vec!["1".to_string()]);
        assert_eq!(sent_ids(&p), vec![vec!["2".to_string()]]);
        assert!(out.error.unwrap().contains("boom"));
    }

    #[test]
    fn dedup_id_is_clipped_to_the_sqs_limit() {
        let long = "x".repeat(200);
        assert_eq!(dedup_id(&long, "n").len(), MAX_DEDUP_ID_LENGTH);
        assert_eq!(dedup_id("job", "nonce"), "job:nonce");
    }
}
