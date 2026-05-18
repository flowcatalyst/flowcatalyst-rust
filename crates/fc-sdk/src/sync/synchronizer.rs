//! `DefinitionSynchronizer` — the orchestrator that pushes a bundled
//! `DefinitionSet` to the platform's per-category sync endpoints.
//!
//! Categories are sync'd in a fixed order so referential FKs already exist
//! by the time later categories arrive:
//!
//! 1. roles
//! 2. event_types
//! 3. subscriptions
//! 4. dispatch_pools
//! 5. principals
//! 6. processes
//!
//! Each category is an independent HTTP call. A failure in one does **not**
//! abort the rest — the error is captured on the returned
//! [`SyncResult`]'s per-category field and the next category proceeds. This
//! mirrors the Laravel SDK's `DefinitionSynchronizer` behaviour and gives
//! operators a single round-trip with full visibility into what
//! succeeded/failed.
//!
//! No filesystem reflection: this SDK requires programmatic builder use.
//! There is intentionally no equivalent of the Laravel `#[AsEventType]`
//! scanner — that path adds a magic dependency without a clear consumer
//! ask.

use super::definition_set::DefinitionSet;
use super::definitions::{
    DispatchPoolDefinition, EventTypeDefinition, PrincipalDefinition, ProcessDefinition,
    RoleDefinition, SubscriptionDefinition,
};
use super::options::SyncOptions;
use super::result::{CategorySyncResult, SyncResult};
use crate::client::{
    ClientError, FlowCatalystClient, SyncDispatchPoolsRequest, SyncEventTypesRequest,
    SyncPrincipalsRequest, SyncResult as WireSyncResult, SyncRolesRequest, SyncSubscriptionsRequest,
};

/// Orchestrates per-category sync calls against a `FlowCatalystClient`.
///
/// Create one via [`DefinitionSynchronizer::new`] and reuse — the
/// underlying HTTP client is `Clone`-cheap and stateless beyond its auth
/// token.
#[derive(Clone)]
pub struct DefinitionSynchronizer {
    client: FlowCatalystClient,
}

impl DefinitionSynchronizer {
    pub fn new(client: FlowCatalystClient) -> Self {
        Self { client }
    }

    /// Sync one application's definitions. Returns a per-category result;
    /// inspect [`SyncResult::has_errors`] / [`SyncResult::errors`] to find
    /// what failed.
    pub async fn sync(&self, set: &DefinitionSet, options: &SyncOptions) -> SyncResult {
        let mut out = SyncResult {
            application_code: set.application_code.clone(),
            ..SyncResult::default()
        };
        let app = set.application_code.as_str();

        if options.sync_roles && set.has_roles() {
            out.roles = Some(self.run_roles(app, &set.roles, options.remove_unlisted).await);
        }
        if options.sync_event_types && set.has_event_types() {
            out.event_types = Some(
                self.run_event_types(app, &set.event_types, options.remove_unlisted)
                    .await,
            );
        }
        if options.sync_subscriptions && set.has_subscriptions() {
            out.subscriptions = Some(
                self.run_subscriptions(app, &set.subscriptions, options.remove_unlisted)
                    .await,
            );
        }
        if options.sync_dispatch_pools && set.has_dispatch_pools() {
            out.dispatch_pools = Some(
                self.run_dispatch_pools(app, &set.dispatch_pools, options.remove_unlisted)
                    .await,
            );
        }
        if options.sync_principals && set.has_principals() {
            out.principals = Some(
                self.run_principals(app, &set.principals, options.remove_unlisted)
                    .await,
            );
        }
        if options.sync_processes && set.has_processes() {
            out.processes = Some(
                self.run_processes(app, &set.processes, options.remove_unlisted)
                    .await,
            );
        }

        out
    }

    /// Sync multiple applications sequentially. Returns one result per
    /// input set, in the same order. Per-set errors do not abort the
    /// chain — each set is independent.
    pub async fn sync_all(
        &self,
        sets: &[DefinitionSet],
        options: &SyncOptions,
    ) -> Vec<SyncResult> {
        let mut results = Vec::with_capacity(sets.len());
        for set in sets {
            results.push(self.sync(set, options).await);
        }
        results
    }

    // ── per-category runners ───────────────────────────────────────────

    async fn run_roles(
        &self,
        app: &str,
        roles: &[RoleDefinition],
        remove_unlisted: bool,
    ) -> CategorySyncResult {
        let req = SyncRolesRequest {
            roles: roles.iter().cloned().map(RoleDefinition::into_wire).collect(),
        };
        match self.client.roles().sync(app, &req, remove_unlisted).await {
            Ok(r) => from_wire(r),
            Err(e) => CategorySyncResult::from_error(format_error(e)),
        }
    }

    async fn run_event_types(
        &self,
        app: &str,
        event_types: &[EventTypeDefinition],
        remove_unlisted: bool,
    ) -> CategorySyncResult {
        let req = SyncEventTypesRequest {
            event_types: event_types
                .iter()
                .cloned()
                .map(EventTypeDefinition::into_wire)
                .collect(),
        };
        match self
            .client
            .event_types()
            .sync(app, &req, remove_unlisted)
            .await
        {
            Ok(r) => from_wire(r),
            Err(e) => CategorySyncResult::from_error(format_error(e)),
        }
    }

    async fn run_subscriptions(
        &self,
        app: &str,
        subscriptions: &[SubscriptionDefinition],
        remove_unlisted: bool,
    ) -> CategorySyncResult {
        let req = SyncSubscriptionsRequest {
            subscriptions: subscriptions
                .iter()
                .cloned()
                .map(SubscriptionDefinition::into_wire)
                .collect(),
        };
        match self
            .client
            .subscriptions()
            .sync(app, &req, remove_unlisted)
            .await
        {
            Ok(r) => from_wire(r),
            Err(e) => CategorySyncResult::from_error(format_error(e)),
        }
    }

    async fn run_dispatch_pools(
        &self,
        app: &str,
        pools: &[DispatchPoolDefinition],
        remove_unlisted: bool,
    ) -> CategorySyncResult {
        let req = SyncDispatchPoolsRequest {
            pools: pools
                .iter()
                .cloned()
                .map(DispatchPoolDefinition::into_wire)
                .collect(),
        };
        match self
            .client
            .dispatch_pools()
            .sync(app, &req, remove_unlisted)
            .await
        {
            Ok(r) => from_wire(r),
            Err(e) => CategorySyncResult::from_error(format_error(e)),
        }
    }

    async fn run_principals(
        &self,
        app: &str,
        principals: &[PrincipalDefinition],
        remove_unlisted: bool,
    ) -> CategorySyncResult {
        let req = SyncPrincipalsRequest {
            principals: principals
                .iter()
                .cloned()
                .map(PrincipalDefinition::into_wire)
                .collect(),
        };
        match self
            .client
            .principals()
            .sync(app, &req, remove_unlisted)
            .await
        {
            Ok(r) => from_wire(r),
            Err(e) => CategorySyncResult::from_error(format_error(e)),
        }
    }

    async fn run_processes(
        &self,
        app: &str,
        processes: &[ProcessDefinition],
        remove_unlisted: bool,
    ) -> CategorySyncResult {
        let inputs = processes
            .iter()
            .cloned()
            .map(ProcessDefinition::into_wire)
            .collect();
        match self
            .client
            .processes()
            .sync(app, inputs, remove_unlisted)
            .await
        {
            Ok(r) => from_wire(r),
            Err(e) => CategorySyncResult::from_error(format_error(e)),
        }
    }
}

fn from_wire(r: WireSyncResult) -> CategorySyncResult {
    CategorySyncResult {
        created: r.created,
        updated: r.updated,
        deleted: r.deleted,
        synced_codes: r.synced_codes,
        error: None,
    }
}

/// Render a `ClientError` as a single-line message suitable for storing
/// on `CategorySyncResult::error`. The API-error variant carries an HTTP
/// status plus the response body, which is often the most informative
/// part of a sync failure.
fn format_error(err: ClientError) -> String {
    err.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::definitions::ProcessDefinition;

    /// Construct a synchronizer pointed at an unreachable host so every
    /// HTTP call fails — we use this to verify the orchestrator's
    /// fail-soft semantics without spinning up a real server.
    fn dummy_synchronizer() -> DefinitionSynchronizer {
        let client = FlowCatalystClient::new("http://127.0.0.1:1") // unbound
            .with_token("test-token");
        DefinitionSynchronizer::new(client)
    }

    #[tokio::test]
    async fn empty_set_skips_every_category() {
        let sync = dummy_synchronizer();
        let set = DefinitionSet::for_application("orders");
        let result = sync.sync(&set, &SyncOptions::defaults()).await;
        assert_eq!(result.application_code, "orders");
        assert!(result.roles.is_none());
        assert!(result.event_types.is_none());
        assert!(result.subscriptions.is_none());
        assert!(result.dispatch_pools.is_none());
        assert!(result.principals.is_none());
        assert!(result.processes.is_none());
        assert!(!result.has_changes());
        assert!(!result.has_errors());
    }

    #[tokio::test]
    async fn category_disabled_via_options_is_skipped_even_when_populated() {
        let sync = dummy_synchronizer();
        let set = DefinitionSet::for_application("orders")
            .add_process(ProcessDefinition::make("orders:f:flow", "Flow"));

        let options = SyncOptions {
            sync_processes: false,
            ..SyncOptions::defaults()
        };
        let result = sync.sync(&set, &options).await;
        assert!(result.processes.is_none());
    }

    #[tokio::test]
    async fn transport_failure_is_captured_per_category_not_propagated() {
        let sync = dummy_synchronizer();
        // Single category populated. The unbound port guarantees ClientError.
        let set = DefinitionSet::for_application("orders")
            .add_process(ProcessDefinition::make("orders:f:flow", "Flow"));
        let result = sync.sync(&set, &SyncOptions::defaults()).await;

        // The processes call ran and recorded an error.
        let processes = result.processes.as_ref().expect("processes ran");
        assert!(processes.is_error());
        assert_eq!(processes.created, 0);

        // Aggregate helpers report the error without panicking.
        assert!(result.has_errors());
        let errors = result.errors();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].0, "processes");
    }

    #[tokio::test]
    async fn sync_all_preserves_input_order() {
        let sync = dummy_synchronizer();
        let sets = vec![
            DefinitionSet::for_application("alpha"),
            DefinitionSet::for_application("beta"),
            DefinitionSet::for_application("gamma"),
        ];
        let results = sync.sync_all(&sets, &SyncOptions::defaults()).await;
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].application_code, "alpha");
        assert_eq!(results[1].application_code, "beta");
        assert_eq!(results[2].application_code, "gamma");
    }
}
