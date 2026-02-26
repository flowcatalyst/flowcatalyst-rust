//! Projection Writers
//!
//! Services for creating and updating read model projections.
//! Projections denormalize data for efficient querying.

use std::sync::Arc;
use tracing::{debug, error, warn};

use crate::{Event, EventRead, DispatchJob, DispatchJobRead};
use crate::{
    EventRepository, DispatchJobRepository,
    ClientRepository, SubscriptionRepository,
};
use crate::shared::error::Result;

/// Event projection writer
/// Creates EventRead projections with denormalized data
pub struct EventProjectionWriter {
    event_repo: Arc<EventRepository>,
    client_repo: Arc<ClientRepository>,
}

impl EventProjectionWriter {
    pub fn new(
        event_repo: Arc<EventRepository>,
        client_repo: Arc<ClientRepository>,
    ) -> Self {
        Self {
            event_repo,
            client_repo,
        }
    }

    /// Create or update projection for an event
    pub async fn project(&self, event: &Event) -> Result<()> {
        // Look up client name if client_id is set
        let client_name = if let Some(ref client_id) = event.client_id {
            match self.client_repo.find_by_id(client_id).await {
                Ok(Some(client)) => Some(client.name),
                Ok(None) => {
                    warn!("Client {} not found for event {}", client_id, event.id);
                    None
                }
                Err(e) => {
                    error!("Error looking up client {}: {:?}", client_id, e);
                    None
                }
            }
        } else {
            None
        };

        // Parse event type code to extract components
        let (application, subdomain, aggregate, event_name) = parse_event_type_code(&event.event_type);

        // Create read projection
        let projection = EventRead {
            id: event.id.clone(),
            event_type: event.event_type.clone(),
            source: event.source.clone(),
            subject: event.subject.clone(),
            time: event.time,
            application,
            subdomain,
            aggregate,
            event_name,
            message_group: event.message_group.clone(),
            correlation_id: event.correlation_id.clone(),
            client_id: event.client_id.clone(),
            client_name,
            created_at: event.created_at,
        };

        // Check if projection exists
        match self.event_repo.find_read_by_id(&event.id).await? {
            Some(_) => {
                self.event_repo.update_read_projection(&projection).await?;
                debug!("Updated event projection {}", event.id);
            }
            None => {
                self.event_repo.insert_read_projection(&projection).await?;
                debug!("Created event projection {}", event.id);
            }
        }

        Ok(())
    }

    /// Project multiple events
    pub async fn project_batch(&self, events: Vec<&Event>) -> Result<usize> {
        let mut count = 0;
        for event in events {
            if let Err(e) = self.project(event).await {
                error!("Failed to project event {}: {:?}", event.id, e);
            } else {
                count += 1;
            }
        }
        Ok(count)
    }
}

/// Dispatch job projection writer
/// Creates DispatchJobRead projections with denormalized data
pub struct DispatchJobProjectionWriter {
    job_repo: Arc<DispatchJobRepository>,
    #[allow(dead_code)]
    client_repo: Arc<ClientRepository>,
    #[allow(dead_code)]
    subscription_repo: Arc<SubscriptionRepository>,
}

impl DispatchJobProjectionWriter {
    pub fn new(
        job_repo: Arc<DispatchJobRepository>,
        client_repo: Arc<ClientRepository>,
        subscription_repo: Arc<SubscriptionRepository>,
    ) -> Self {
        Self {
            job_repo,
            client_repo,
            subscription_repo,
        }
    }

    /// Create or update projection for a dispatch job
    pub async fn project(&self, job: &DispatchJob) -> Result<()> {
        // Create read projection from job - use the From impl
        let projection = DispatchJobRead::from(job);

        // Check if projection exists
        match self.job_repo.find_read_by_id(&job.id).await? {
            Some(_) => {
                self.job_repo.update_read_projection(&projection).await?;
                debug!("Updated dispatch job projection {}", job.id);
            }
            None => {
                self.job_repo.insert_read_projection(&projection).await?;
                debug!("Created dispatch job projection {}", job.id);
            }
        }

        Ok(())
    }

    /// Project multiple jobs
    pub async fn project_batch(&self, jobs: Vec<&DispatchJob>) -> Result<usize> {
        let mut count = 0;
        for job in jobs {
            if let Err(e) = self.project(job).await {
                error!("Failed to project job {}: {:?}", job.id, e);
            } else {
                count += 1;
            }
        }
        Ok(count)
    }
}

/// Parse event type code into components
/// Format: {application}:{subdomain}:{aggregate}:{event}
fn parse_event_type_code(code: &str) -> (Option<String>, Option<String>, Option<String>, Option<String>) {
    let parts: Vec<&str> = code.split(':').collect();
    match parts.len() {
        4 => (
            Some(parts[0].to_string()),
            Some(parts[1].to_string()),
            Some(parts[2].to_string()),
            Some(parts[3].to_string()),
        ),
        3 => (
            Some(parts[0].to_string()),
            Some(parts[1].to_string()),
            Some(parts[2].to_string()),
            None,
        ),
        2 => (
            Some(parts[0].to_string()),
            Some(parts[1].to_string()),
            None,
            None,
        ),
        1 if !parts[0].is_empty() => (
            Some(parts[0].to_string()),
            None,
            None,
            None,
        ),
        _ => (None, None, None, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_event_type_code_full() {
        let (app, sub, agg, evt) = parse_event_type_code("orders:fulfillment:shipment:shipped");
        assert_eq!(app, Some("orders".to_string()));
        assert_eq!(sub, Some("fulfillment".to_string()));
        assert_eq!(agg, Some("shipment".to_string()));
        assert_eq!(evt, Some("shipped".to_string()));
    }

    #[test]
    fn test_parse_event_type_code_partial() {
        let (app, sub, agg, evt) = parse_event_type_code("orders:fulfillment");
        assert_eq!(app, Some("orders".to_string()));
        assert_eq!(sub, Some("fulfillment".to_string()));
        assert_eq!(agg, None);
        assert_eq!(evt, None);
    }

    #[test]
    fn test_parse_event_type_code_single() {
        let (app, sub, agg, evt) = parse_event_type_code("orders");
        assert_eq!(app, Some("orders".to_string()));
        assert_eq!(sub, None);
        assert_eq!(agg, None);
        assert_eq!(evt, None);
    }

    #[test]
    fn test_parse_event_type_code_empty() {
        let (app, sub, agg, evt) = parse_event_type_code("");
        assert_eq!(app, None);
        assert_eq!(sub, None);
        assert_eq!(agg, None);
        assert_eq!(evt, None);
    }
}
