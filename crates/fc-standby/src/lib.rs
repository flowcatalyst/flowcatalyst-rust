//! FlowCatalyst Standby Mode
//!
//! Provides Redis-based leader election for running multiple instances
//! of FlowCatalyst with only one actively processing messages.
//!
//! # Features
//!
//! - **Leader Election**: Redis-based distributed lock with lease renewal
//! - **Automatic Failover**: If leader fails, another instance takes over
//! - **Standby Guard**: Helper to gate operations on leadership status
//!
//! # Example
//!
//! ```no_run
//! use fc_standby::{LeaderElection, LeaderElectionConfig, StandbyGuard};
//! use std::sync::Arc;
//!
//! async fn example() {
//!     let config = LeaderElectionConfig::new("redis://localhost:6379".to_string())
//!         .with_lock_key("my-service:leader".to_string());
//!
//!     let election = Arc::new(LeaderElection::new(config).await.unwrap());
//!     election.clone().start().await.unwrap();
//!
//!     let guard = StandbyGuard::new(election.clone());
//!
//!     // Only process if we're the leader
//!     if guard.should_process() {
//!         // Do work
//!     }
//!
//!     // Or run a closure only if leader
//!     guard.run_if_leader(|| async {
//!         println!("I'm the leader!");
//!     }).await;
//! }
//! ```

mod error;
mod leader;

pub use error::{StandbyError, Result};
pub use leader::{LeaderElection, LeaderElectionConfig, LeadershipStatus, StandbyGuard};
