//! Outbox Manager
//!
//! High-level manager for creating outbox messages. This is the simpler
//! alternative to the full [`UnitOfWork`](super::UnitOfWork) pattern —
//! use it when you don't need atomic entity + event commits.
//!
//! Matches the TS and Laravel SDK `OutboxManager` API.
//!
//! # Example
//!
//! ```ignore
//! use fc_sdk::outbox::{OutboxManager, SqlxPgDriver, CreateEventDto, CreateDispatchJobDto};
//!
//! let driver = SqlxPgDriver::new(pool);
//! let outbox = OutboxManager::new(Box::new(driver), "clt_0HZXEQ5Y8JY5Z");
//!
//! // Create a single event
//! let event_id = outbox.create_event(
//!     CreateEventDto::new("user.registered", serde_json::json!({"userId": "123"}))
//!         .source("user-service")
//!         .message_group("users:user:123"),
//! ).await?;
//!
//! // Create a batch of dispatch jobs
//! let job_ids = outbox.create_dispatch_jobs(vec![job1, job2]).await?;
//! ```

use crate::tsid;

use super::driver::{MessageType, OutboxDriver, OutboxMessage, OutboxStatus};
use super::dto::{CreateAuditLogDto, CreateDispatchJobDto, CreateEventDto};
use super::error::OutboxError;

/// Manages outbox message creation.
pub struct OutboxManager {
    driver: Box<dyn OutboxDriver>,
    client_id: String,
}

impl OutboxManager {
    pub fn new(driver: Box<dyn OutboxDriver>, client_id: impl Into<String>) -> Self {
        Self {
            driver,
            client_id: client_id.into(),
        }
    }

    /// Create a single event in the outbox. Returns the generated TSID.
    pub async fn create_event(&self, event: CreateEventDto) -> Result<String, OutboxError> {
        self.ensure_client_id()?;

        let id = tsid::generate_untyped();
        let payload = serde_json::to_string(&event.to_payload())?;
        let headers = if event.headers.is_empty() {
            None
        } else {
            Some(event.headers.clone())
        };

        let message = self.build_message(
            id.clone(),
            MessageType::Event,
            &payload,
            event.message_group.as_deref(),
            headers,
        );

        self.driver.insert(message).await?;
        Ok(id)
    }

    /// Create multiple events in the outbox (batch). Returns the generated TSIDs.
    pub async fn create_events(
        &self,
        events: Vec<CreateEventDto>,
    ) -> Result<Vec<String>, OutboxError> {
        if events.is_empty() {
            return Ok(Vec::new());
        }
        self.ensure_client_id()?;

        let mut ids = Vec::with_capacity(events.len());
        let mut messages = Vec::with_capacity(events.len());

        for event in &events {
            let id = tsid::generate_untyped();
            ids.push(id.clone());
            let payload = serde_json::to_string(&event.to_payload())?;
            let headers = if event.headers.is_empty() {
                None
            } else {
                Some(event.headers.clone())
            };

            messages.push(self.build_message(
                id,
                MessageType::Event,
                &payload,
                event.message_group.as_deref(),
                headers,
            ));
        }

        self.driver.insert_batch(messages).await?;
        Ok(ids)
    }

    /// Create a single dispatch job in the outbox. Returns the generated TSID.
    pub async fn create_dispatch_job(
        &self,
        job: CreateDispatchJobDto,
    ) -> Result<String, OutboxError> {
        self.ensure_client_id()?;

        let id = tsid::generate_untyped();
        let payload = serde_json::to_string(&job.to_payload())?;

        let message = self.build_message(
            id.clone(),
            MessageType::DispatchJob,
            &payload,
            job.message_group.as_deref(),
            None,
        );

        self.driver.insert(message).await?;
        Ok(id)
    }

    /// Create multiple dispatch jobs in the outbox (batch). Returns the generated TSIDs.
    pub async fn create_dispatch_jobs(
        &self,
        jobs: Vec<CreateDispatchJobDto>,
    ) -> Result<Vec<String>, OutboxError> {
        if jobs.is_empty() {
            return Ok(Vec::new());
        }
        self.ensure_client_id()?;

        let mut ids = Vec::with_capacity(jobs.len());
        let mut messages = Vec::with_capacity(jobs.len());

        for job in &jobs {
            let id = tsid::generate_untyped();
            ids.push(id.clone());
            let payload = serde_json::to_string(&job.to_payload())?;

            messages.push(self.build_message(
                id,
                MessageType::DispatchJob,
                &payload,
                job.message_group.as_deref(),
                None,
            ));
        }

        self.driver.insert_batch(messages).await?;
        Ok(ids)
    }

    /// Create a single audit log in the outbox. Returns the generated TSID.
    pub async fn create_audit_log(&self, audit: CreateAuditLogDto) -> Result<String, OutboxError> {
        self.ensure_client_id()?;

        let id = tsid::generate_untyped();
        let payload = serde_json::to_string(&audit.to_payload())?;
        let headers = if audit.headers.is_empty() {
            None
        } else {
            Some(audit.headers.clone())
        };

        let message =
            self.build_message(id.clone(), MessageType::AuditLog, &payload, None, headers);

        self.driver.insert(message).await?;
        Ok(id)
    }

    /// Create multiple audit logs in the outbox (batch). Returns the generated TSIDs.
    pub async fn create_audit_logs(
        &self,
        audits: Vec<CreateAuditLogDto>,
    ) -> Result<Vec<String>, OutboxError> {
        if audits.is_empty() {
            return Ok(Vec::new());
        }
        self.ensure_client_id()?;

        let mut ids = Vec::with_capacity(audits.len());
        let mut messages = Vec::with_capacity(audits.len());

        for audit in &audits {
            let id = tsid::generate_untyped();
            ids.push(id.clone());
            let payload = serde_json::to_string(&audit.to_payload())?;
            let headers = if audit.headers.is_empty() {
                None
            } else {
                Some(audit.headers.clone())
            };

            messages.push(self.build_message(id, MessageType::AuditLog, &payload, None, headers));
        }

        self.driver.insert_batch(messages).await?;
        Ok(ids)
    }

    /// Get the underlying driver.
    pub fn driver(&self) -> &dyn OutboxDriver {
        &*self.driver
    }

    fn build_message(
        &self,
        id: String,
        message_type: MessageType,
        payload: &str,
        message_group: Option<&str>,
        headers: Option<std::collections::HashMap<String, String>>,
    ) -> OutboxMessage {
        let now = chrono::Utc::now().to_rfc3339();
        OutboxMessage {
            id,
            message_type,
            message_group: message_group.map(|s| s.to_string()),
            payload: payload.to_string(),
            status: OutboxStatus::Pending,
            created_at: now.clone(),
            updated_at: now,
            client_id: self.client_id.clone(),
            payload_size: payload.len() as i32,
            headers,
        }
    }

    fn ensure_client_id(&self) -> Result<(), OutboxError> {
        if self.client_id.is_empty() {
            return Err(OutboxError::MissingClientId);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// In-memory driver that captures inserted messages for assertion.
    struct MockDriver {
        messages: Arc<Mutex<Vec<OutboxMessage>>>,
    }

    impl MockDriver {
        fn new() -> (Self, Arc<Mutex<Vec<OutboxMessage>>>) {
            let messages = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    messages: messages.clone(),
                },
                messages,
            )
        }
    }

    #[async_trait::async_trait]
    impl OutboxDriver for MockDriver {
        async fn insert(&self, message: OutboxMessage) -> Result<(), OutboxError> {
            self.messages.lock().unwrap().push(message);
            Ok(())
        }
        async fn insert_batch(&self, messages: Vec<OutboxMessage>) -> Result<(), OutboxError> {
            self.messages.lock().unwrap().extend(messages);
            Ok(())
        }
    }

    fn make_manager(client_id: &str) -> (OutboxManager, Arc<Mutex<Vec<OutboxMessage>>>) {
        let (driver, captured) = MockDriver::new();
        let mgr = OutboxManager::new(Box::new(driver), client_id);
        (mgr, captured)
    }

    #[tokio::test]
    async fn create_event_inserts_message_with_correct_fields() {
        let (mgr, captured) = make_manager("clt_test");
        let dto = CreateEventDto::new("user.registered", serde_json::json!({"id": "u1"}))
            .source("user-svc")
            .message_group("users:user:u1");

        let id = mgr.create_event(dto).await.unwrap();
        assert!(!id.is_empty());

        let msgs = captured.lock().unwrap();
        assert_eq!(msgs.len(), 1);

        let msg = &msgs[0];
        assert_eq!(msg.id, id);
        assert_eq!(msg.message_type, MessageType::Event);
        assert_eq!(msg.message_group.as_deref(), Some("users:user:u1"));
        assert_eq!(msg.status, OutboxStatus::Pending);
        assert_eq!(msg.client_id, "clt_test");
        assert!(msg.payload_size > 0);
        assert!(msg.headers.is_none()); // no headers on DTO

        // Verify payload is valid JSON containing the event
        let payload: serde_json::Value = serde_json::from_str(&msg.payload).unwrap();
        assert_eq!(payload["type"], "user.registered");
        assert_eq!(payload["source"], "user-svc");
    }

    #[tokio::test]
    async fn create_event_with_headers_propagates_them() {
        let (mgr, captured) = make_manager("clt_1");
        let mut headers = std::collections::HashMap::new();
        headers.insert("X-Trace".to_string(), "abc".to_string());

        let dto = CreateEventDto::new("t", serde_json::json!({})).headers(headers);
        mgr.create_event(dto).await.unwrap();

        let msgs = captured.lock().unwrap();
        let msg = &msgs[0];
        let h = msg.headers.as_ref().unwrap();
        assert_eq!(h.get("X-Trace").unwrap(), "abc");
    }

    #[tokio::test]
    async fn create_events_batch_returns_ids_and_inserts_all() {
        let (mgr, captured) = make_manager("clt_batch");
        let events = vec![
            CreateEventDto::new("e1", serde_json::json!({})),
            CreateEventDto::new("e2", serde_json::json!({})),
            CreateEventDto::new("e3", serde_json::json!({})),
        ];

        let ids = mgr.create_events(events).await.unwrap();
        assert_eq!(ids.len(), 3);

        let msgs = captured.lock().unwrap();
        assert_eq!(msgs.len(), 3);
        // IDs match
        for (i, msg) in msgs.iter().enumerate() {
            assert_eq!(msg.id, ids[i]);
            assert_eq!(msg.message_type, MessageType::Event);
            assert_eq!(msg.client_id, "clt_batch");
        }
    }

    #[tokio::test]
    async fn create_events_empty_returns_empty() {
        let (mgr, captured) = make_manager("clt_x");
        let ids = mgr.create_events(vec![]).await.unwrap();
        assert!(ids.is_empty());
        assert!(captured.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn create_dispatch_job_inserts_correct_type() {
        let (mgr, captured) = make_manager("clt_dj");
        let dto = CreateDispatchJobDto::new("svc", "code", "https://x.com", "{}", "pool_1")
            .message_group("grp:1");

        let id = mgr.create_dispatch_job(dto).await.unwrap();
        assert!(!id.is_empty());

        let msgs = captured.lock().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].message_type, MessageType::DispatchJob);
        assert_eq!(msgs[0].message_group.as_deref(), Some("grp:1"));
        assert_eq!(msgs[0].client_id, "clt_dj");
        // Dispatch jobs don't pass headers
        assert!(msgs[0].headers.is_none());
    }

    #[tokio::test]
    async fn create_dispatch_jobs_batch() {
        let (mgr, captured) = make_manager("clt_djb");
        let jobs = vec![
            CreateDispatchJobDto::new("s", "c1", "u", "{}", "p"),
            CreateDispatchJobDto::new("s", "c2", "u", "{}", "p"),
        ];

        let ids = mgr.create_dispatch_jobs(jobs).await.unwrap();
        assert_eq!(ids.len(), 2);

        let msgs = captured.lock().unwrap();
        assert_eq!(msgs.len(), 2);
        for msg in msgs.iter() {
            assert_eq!(msg.message_type, MessageType::DispatchJob);
        }
    }

    #[tokio::test]
    async fn create_dispatch_jobs_empty() {
        let (mgr, captured) = make_manager("clt_x");
        let ids = mgr.create_dispatch_jobs(vec![]).await.unwrap();
        assert!(ids.is_empty());
        assert!(captured.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn create_audit_log_inserts_correct_type() {
        let (mgr, captured) = make_manager("clt_al");
        let dto = CreateAuditLogDto::new("Client", "clt_1", "CREATE").principal_id("prn_admin");

        let id = mgr.create_audit_log(dto).await.unwrap();
        assert!(!id.is_empty());

        let msgs = captured.lock().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].message_type, MessageType::AuditLog);
        // Audit logs have no message_group
        assert!(msgs[0].message_group.is_none());
        assert_eq!(msgs[0].client_id, "clt_al");
    }

    #[tokio::test]
    async fn create_audit_log_with_headers() {
        let (mgr, captured) = make_manager("clt_al2");
        let mut headers = std::collections::HashMap::new();
        headers.insert("X-Audit".to_string(), "true".to_string());

        let dto = CreateAuditLogDto::new("E", "e_1", "DELETE").headers(headers);
        mgr.create_audit_log(dto).await.unwrap();

        let msgs = captured.lock().unwrap();
        assert!(msgs[0].headers.is_some());
        assert_eq!(msgs[0].headers.as_ref().unwrap()["X-Audit"], "true");
    }

    #[tokio::test]
    async fn create_audit_logs_batch() {
        let (mgr, captured) = make_manager("clt_alb");
        let audits = vec![
            CreateAuditLogDto::new("A", "a_1", "CREATE"),
            CreateAuditLogDto::new("B", "b_1", "UPDATE"),
        ];

        let ids = mgr.create_audit_logs(audits).await.unwrap();
        assert_eq!(ids.len(), 2);

        let msgs = captured.lock().unwrap();
        assert_eq!(msgs.len(), 2);
        for msg in msgs.iter() {
            assert_eq!(msg.message_type, MessageType::AuditLog);
        }
    }

    #[tokio::test]
    async fn create_audit_logs_empty() {
        let (mgr, captured) = make_manager("clt_x");
        let ids = mgr.create_audit_logs(vec![]).await.unwrap();
        assert!(ids.is_empty());
        assert!(captured.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn empty_client_id_returns_error() {
        let (mgr, _) = make_manager("");

        let err = mgr
            .create_event(CreateEventDto::new("t", serde_json::json!({})))
            .await;
        assert!(err.is_err());
        assert!(err
            .unwrap_err()
            .to_string()
            .contains("client_id is required"));

        let (mgr2, _) = make_manager("");
        let err2 = mgr2
            .create_dispatch_job(CreateDispatchJobDto::new("s", "c", "u", "{}", "p"))
            .await;
        assert!(err2.is_err());

        let (mgr3, _) = make_manager("");
        let err3 = mgr3
            .create_audit_log(CreateAuditLogDto::new("E", "e", "OP"))
            .await;
        assert!(err3.is_err());
    }

    #[tokio::test]
    async fn payload_size_matches_payload_length() {
        let (mgr, captured) = make_manager("clt_ps");
        let dto = CreateEventDto::new(
            "test.event",
            serde_json::json!({"big_field": "a]".repeat(100)}),
        );
        mgr.create_event(dto).await.unwrap();

        let msgs = captured.lock().unwrap();
        let msg = &msgs[0];
        assert_eq!(msg.payload_size, msg.payload.len() as i32);
    }

    #[tokio::test]
    async fn created_at_and_updated_at_are_rfc3339() {
        let (mgr, captured) = make_manager("clt_ts");
        mgr.create_event(CreateEventDto::new("t", serde_json::json!({})))
            .await
            .unwrap();

        let msgs = captured.lock().unwrap();
        let msg = &msgs[0];
        // Should parse as valid RFC3339
        let _: chrono::DateTime<chrono::Utc> = msg.created_at.parse().unwrap();
        let _: chrono::DateTime<chrono::Utc> = msg.updated_at.parse().unwrap();
        assert_eq!(msg.created_at, msg.updated_at);
    }

    #[tokio::test]
    async fn unique_ids_across_messages() {
        let (mgr, captured) = make_manager("clt_uid");
        let events = vec![
            CreateEventDto::new("a", serde_json::json!({})),
            CreateEventDto::new("b", serde_json::json!({})),
            CreateEventDto::new("c", serde_json::json!({})),
        ];
        mgr.create_events(events).await.unwrap();

        let msgs = captured.lock().unwrap();
        let ids: Vec<&str> = msgs.iter().map(|m| m.id.as_str()).collect();
        let unique: std::collections::HashSet<&&str> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "all IDs must be unique");
    }
}
