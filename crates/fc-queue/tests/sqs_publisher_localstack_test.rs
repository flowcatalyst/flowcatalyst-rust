//! The SQS FIFO publisher against a real SQS emulator (LocalStack, via
//! testcontainers). Ignored by default; needs Docker and the
//! `localstack/localstack:3.0` image:
//!   cargo test -p fc-queue --features sqs --test sqs_publisher_localstack_test -- --ignored

#![cfg(feature = "sqs")]
// `attribute_names`, which LocalStack 3.0 still needs.
#![allow(deprecated)]

use std::collections::HashSet;

use aws_config::{BehaviorVersion, Region};
use aws_sdk_sqs::types::MessageSystemAttributeName;
use aws_sdk_sqs::Client;
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, ImageExt};
use testcontainers_modules::localstack::LocalStack;

use fc_queue::sqs_publisher::{AwsSqsBatchApi, FifoPublishItem, QueueAddressing, SqsFifoPublisher};

const ACCOUNT: &str = "000000000000";
const REGION: &str = "us-east-1";

async fn start() -> (ContainerAsync<LocalStack>, Client) {
    let container = LocalStack::default()
        .with_tag("3.0")
        .with_env_var("SERVICES", "sqs")
        .start()
        .await
        .expect("start localstack");
    let port = container.get_host_port_ipv4(4566).await.unwrap();
    let host = container.get_host().await.unwrap();
    let endpoint = format!("http://{host}:{port}");
    let config = aws_config::defaults(BehaviorVersion::latest())
        .region(Region::new(REGION))
        .endpoint_url(&endpoint)
        .credentials_provider(aws_sdk_sqs::config::Credentials::new(
            "test", "test", None, None, "test",
        ))
        .load()
        .await;
    (container, Client::new(&config))
}

fn item(id: &str, queue: &str, group: &str) -> FifoPublishItem {
    FifoPublishItem {
        id: id.to_string(),
        queue_name: queue.to_string(),
        group_id: group.to_string(),
        body: format!(r#"{{"id":"{id}","mediationType":"HTTP","mediationTarget":"http://x"}}"#),
    }
}

async fn receive_all(client: &Client, url: &str, want: usize) -> Vec<(String, String)> {
    let mut got = Vec::new();
    for _ in 0..20 {
        let out = client
            .receive_message()
            .queue_url(url)
            .max_number_of_messages(10)
            .message_system_attribute_names(MessageSystemAttributeName::MessageGroupId)
            // LocalStack 3.0 answers the older attribute selector only.
            .attribute_names(aws_sdk_sqs::types::QueueAttributeName::All)
            .wait_time_seconds(1)
            .send()
            .await
            .expect("receive");
        for m in out.messages() {
            let group = m
                .attributes()
                .and_then(|a| a.get(&MessageSystemAttributeName::MessageGroupId))
                .cloned()
                .unwrap_or_default();
            got.push((m.body().unwrap_or_default().to_string(), group));
            client
                .delete_message()
                .queue_url(url)
                .receipt_handle(m.receipt_handle().unwrap())
                .send()
                .await
                .expect("delete");
        }
        if got.len() >= want {
            break;
        }
    }
    got
}

/// A queue that does not exist is created as FIFO on first publish, the
/// messages arrive with their group ids, and publishing the same id again
/// (stale recovery, a retry) is not swallowed by FIFO deduplication.
#[tokio::test]
#[ignore]
async fn creates_the_fifo_queue_lazily_and_republishes_the_same_id() {
    let (_container, client) = start().await;
    let addressing = QueueAddressing::Composed {
        region: REGION.to_string(),
        account_id: ACCOUNT.to_string(),
    };
    let publisher = SqsFifoPublisher::new(AwsSqsBatchApi::new(client.clone()), addressing.clone());
    let queue = "FC-test-acme-DEFAULT.fifo";

    let out = publisher
        .publish(vec![
            item("job1", queue, "orders-1"),
            item("job2", queue, "orders-1"),
            item("job3", queue, "job3"),
        ])
        .await;
    assert!(out.unpublished.is_empty(), "{out:?}");

    let url = client
        .get_queue_url()
        .queue_name(queue)
        .send()
        .await
        .expect("the queue was created")
        .queue_url()
        .unwrap()
        .to_string();
    let attrs = client
        .get_queue_attributes()
        .queue_url(&url)
        .attribute_names(aws_sdk_sqs::types::QueueAttributeName::All)
        .send()
        .await
        .unwrap();
    let attrs = attrs.attributes().unwrap();
    assert_eq!(
        attrs
            .get(&aws_sdk_sqs::types::QueueAttributeName::FifoQueue)
            .map(String::as_str),
        Some("true")
    );
    assert_eq!(
        attrs
            .get(&aws_sdk_sqs::types::QueueAttributeName::ContentBasedDeduplication)
            .map(String::as_str),
        Some("false")
    );

    let got = receive_all(&client, &url, 3).await;
    let ids: HashSet<String> = got
        .iter()
        .map(|(b, _)| {
            serde_json::from_str::<serde_json::Value>(b).unwrap()["id"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(
        ids,
        ["job1", "job2", "job3"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    );
    for (body, group) in &got {
        let id = serde_json::from_str::<serde_json::Value>(body).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let want = if id == "job3" { "job3" } else { "orders-1" };
        assert_eq!(group, want, "group id of {id}");
    }

    // Publishing job1 again within the five-minute dedup window is a second,
    // intended delivery.
    let out = publisher
        .publish(vec![item("job1", queue, "orders-1")])
        .await;
    assert!(out.unpublished.is_empty(), "{out:?}");
    let again = receive_all(&client, &url, 1).await;
    assert_eq!(
        again.len(),
        1,
        "the re-publish must not be deduplicated away"
    );
}
