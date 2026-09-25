//! Broker backends for the router binaries (feature `backends`): the
//! consumer factory that dispatches on a queue URI's scheme, and the SQS
//! publisher behind the router API's publish endpoint. One copy, used by
//! the standalone `fc-router` binary and `fc-server`'s router role.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use fc_common::{Message, QueueConfig};
use fc_queue::{QueueError, QueuePublisher, QueueScheme};
use tracing::info;

use crate::manager::{ConsumerFactory, QueueManager};
use crate::RouterError;

/// An SQS client from the default AWS chain (`AWS_REGION`, the task role,
/// `AWS_ENDPOINT_URL[_SQS]`). In dev mode it points at LocalStack
/// (`LOCALSTACK_ENDPOINT`, default `http://localhost:4566`).
pub async fn sqs_client(dev_mode: bool) -> aws_sdk_sqs::Client {
    let config = if dev_mode {
        let endpoint_url = std::env::var("LOCALSTACK_ENDPOINT")
            .unwrap_or_else(|_| "http://localhost:4566".to_string());
        info!(endpoint = %endpoint_url, "Configuring SQS client for LocalStack");
        aws_config::defaults(aws_config::BehaviorVersion::latest())
            .endpoint_url(&endpoint_url)
            .load()
            .await
    } else {
        aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await
    };
    aws_sdk_sqs::Client::new(&config)
}

/// Builds a consumer for each configured queue from its URI's scheme
/// (`docs/spec/router.md` §7.1): `http(s)://sqs.<region>.amazonaws.com/…`
/// → SQS, `nats://` → NATS JetStream, `postgres://` → the Postgres queue.
pub struct SchemeConsumerFactory {
    sqs_client: aws_sdk_sqs::Client,
}

impl SchemeConsumerFactory {
    pub fn new(sqs_client: aws_sdk_sqs::Client) -> Self {
        Self { sqs_client }
    }
}

#[async_trait]
impl ConsumerFactory for SchemeConsumerFactory {
    async fn create_consumer(
        &self,
        config: &QueueConfig,
    ) -> Result<Arc<dyn fc_queue::QueueConsumer>, RouterError> {
        let scheme = fc_queue::resolve_scheme(&config.uri)
            .map_err(RouterError::consumer(&config.name, "resolve queue scheme"))?;

        match scheme {
            QueueScheme::Sqs => {
                info!(
                    queue_name = %config.name,
                    queue_uri = %config.uri,
                    visibility_timeout = config.visibility_timeout,
                    "Creating SQS consumer from config"
                );
                let consumer = fc_queue::sqs::SqsQueueConsumer::from_queue_url(
                    self.sqs_client.clone(),
                    config.uri.clone(),
                    config.visibility_timeout as i32,
                )
                .await;
                Ok(Arc::new(consumer))
            }
            QueueScheme::Nats => build_nats_consumer(config).await,
            QueueScheme::Postgres => build_postgres_consumer(config).await,
        }
    }
}

/// A NATS JetStream consumer from a `nats://` queue URI (§7.4): stream,
/// consumer and subject come from the URI's query string.
async fn build_nats_consumer(
    config: &QueueConfig,
) -> Result<Arc<dyn fc_queue::QueueConsumer>, RouterError> {
    let nats_config = fc_queue::nats::NatsConfig::from_uri(&config.uri)
        .map_err(RouterError::consumer(&config.name, "invalid NATS URI"))?;
    info!(
        queue_name = %config.name,
        stream = %nats_config.stream_name,
        consumer = %nats_config.consumer_name,
        subject = %nats_config.subject,
        "Creating NATS JetStream consumer from config"
    );
    let consumer = fc_queue::nats::NatsQueueConsumer::new(nats_config)
        .await
        .map_err(RouterError::consumer(
            &config.name,
            "NATS consumer setup failed",
        ))?;
    Ok(Arc::new(consumer))
}

/// A Postgres queue consumer from a `postgres://` queue URI: the URI carries
/// its own connection info (§7.3, Go `pgxpool.New(ctx, cfg.URI)`), so each
/// queue gets its own pool, sized as Go's `max(4, NumCPU)` (a smaller pool
/// starved acks under load — see
/// `fc_queue::postgres::default_max_connections`). The config's name is the
/// consumer's identifier.
async fn build_postgres_consumer(
    config: &QueueConfig,
) -> Result<Arc<dyn fc_queue::QueueConsumer>, RouterError> {
    let max_connections = fc_queue::postgres::default_max_connections();
    info!(
        queue_name = %config.name,
        max_connections,
        "Creating Postgres queue consumer from config"
    );
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&config.uri)
        .await
        .map_err(RouterError::consumer(
            &config.name,
            "Postgres pool connect failed",
        ))?;

    let visibility = if config.visibility_timeout == 0 {
        30
    } else {
        config.visibility_timeout
    };
    let consumer = fc_queue::postgres::PostgresQueue::new(pool, config.name.clone(), visibility);

    use fc_queue::EmbeddedQueue;
    consumer.init_schema().await.map_err(RouterError::consumer(
        &config.name,
        "Postgres schema init failed",
    ))?;

    Ok(Arc::new(consumer))
}

/// Publishes to an SQS queue chosen when it publishes, the way Go's
/// `Manager.Publisher` resolves one: the lowest-named SQS queue the manager
/// currently runs, falling back to `fallback_url` (dev mode's first queue).
pub struct SqsPublisher {
    client: aws_sdk_sqs::Client,
    manager: Arc<QueueManager>,
    fallback_url: Option<String>,
}

impl SqsPublisher {
    pub fn new(
        client: aws_sdk_sqs::Client,
        manager: Arc<QueueManager>,
        fallback_url: Option<String>,
    ) -> Self {
        Self {
            client,
            manager,
            fallback_url,
        }
    }

    fn queue_url(&self) -> fc_queue::Result<String> {
        let mut sqs: Vec<(String, String)> = self
            .manager
            .queue_configs()
            .into_iter()
            .filter(|(_, c)| matches!(fc_queue::resolve_scheme(&c.uri), Ok(QueueScheme::Sqs)))
            .map(|(name, c)| (name, c.uri))
            .collect();
        sqs.sort();
        sqs.into_iter()
            .next()
            .map(|(_, uri)| uri)
            .or_else(|| self.fallback_url.clone())
            .ok_or_else(|| QueueError::Config("publisher: no SQS queue registered".to_string()))
    }
}

#[async_trait]
impl QueuePublisher for SqsPublisher {
    fn identifier(&self) -> &str {
        "sqs-publisher"
    }

    async fn publish(&self, message: Message) -> fc_queue::Result<String> {
        let message_id = message.id.clone();
        let body = serde_json::to_string(&message)?;
        let queue_url = self.queue_url()?;

        let mut request = self
            .client
            .send_message()
            .queue_url(&queue_url)
            .message_body(body);

        // FIFO queues require message_group_id and message_deduplication_id
        if queue_url.ends_with(".fifo") {
            let group_id = message
                .message_group_id
                .clone()
                .unwrap_or_else(|| "default".to_string());
            request = request
                .message_group_id(group_id)
                .message_deduplication_id(&message_id);
        }

        request.send().await.map_err(QueueError::sqs)?;

        Ok(message_id)
    }

    async fn publish_batch(&self, messages: Vec<Message>) -> fc_queue::Result<Vec<String>> {
        let mut ids = Vec::with_capacity(messages.len());
        for message in messages {
            let id = self.publish(message).await?;
            ids.push(id);
        }
        Ok(ids)
    }
}
