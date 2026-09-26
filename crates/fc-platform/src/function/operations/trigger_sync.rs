//! Where a function's live manifest becomes platform-managed wiring (Java
//! `function/operations/TriggerSync.java` and `FunctionTriggerSync.java`,
//! spec `function-invocation.md` §4): its dispatch pool `fn-<fid>`, one
//! subscription per event type (source `FUNCTION`), one scheduled job per
//! `(cron, timezone)`, a `fn_trigger_objects` link for each, and `fn_routes`
//! for its public routes.
//!
//! - [`TriggerSync::plan`] reads current state and says what promoting a
//!   manifest would do ([`PromotePlan`]). No writes. Promote and the
//!   `manifest/check` dry run share it.
//! - [`TriggerSync::apply`] performs exactly a plan: the pool first, then
//!   each subscription and schedule to create or update, deletions last
//!   (subscriptions, then schedules). No difference, no write, no event.
//!   The public routes are the promote's own commit
//!   ([`TriggerSync::routes_for`]): they are the `live` alias's projection,
//!   written with the alias change.
//! - [`TriggerSync::on_delete`] deletes every linked subscription, job and
//!   the pool before the function row goes.
//! - [`TriggerSync::on_status_change`] pauses the `ACTIVE` linked
//!   subscriptions and jobs when the function is disabled, and resumes the
//!   `PAUSED` ones when it is enabled; one already in the target state (an
//!   operator paused it by hand) is left alone. The pool never changes.
//!
//! Every write goes through the target aggregate's own repository with that
//! aggregate's own event and command, as Java commits them: a function's
//! subscription is a real subscription with a real audit trail. The link
//! is written beside it in the same commit ([`LinkedRepository`]). All of it
//! runs on the caller's unit of work, which for promote, delete and update
//! is one transaction (`PgUnitOfWork::run`), as Java's `TxOperation`s: a
//! write that cannot be honoured rolls the whole operation back.
//!
//! Reads are batched: the links in one query, then each kind's objects in
//! one query each.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use sha2::{Digest as _, Sha256};

use super::access::Caller;
use super::promote_plan::{
    Conflict, PoolAction, PromotePlan, PublicRoutesAction, RouteKey, ScheduleAction,
    SubscriptionAction, Wiring,
};
use super::publish_checks::routes_taken;
use crate::dispatch_pool::operations::{
    CreateDispatchPoolCommand, DeleteDispatchPoolCommand, DispatchPoolCreated, DispatchPoolDeleted,
    DispatchPoolUpdated, UpdateDispatchPoolCommand,
};
use crate::function::dispatch_mode;
use crate::function::entity::{
    Function, FunctionRoute, FunctionStatus, FunctionVersion, TriggerObject, TriggerObjectKind,
};
use crate::function::repository::FunctionRepository;
use crate::function::route_repository::FunctionRouteRepository;
use crate::function::schedule_check::parse_java_cron;
use crate::function::settings_repository::FunctionSettingsRepository;
use crate::function::trigger_object_repository::{
    Linked, LinkedRepository, TriggerObjectRepository,
};
use crate::function::version_repository::FunctionVersionRepository;
use crate::function::{Manifest, PoolUrlTemplate, ScheduleSpec, SubscriptionSpec, LIVE_ALIAS};
use crate::scheduled_job::entity::{JobDefinition, ScheduledJob, ScheduledJobStatus};
use crate::scheduled_job::operations::events::{
    ScheduledJobCreated, ScheduledJobDeleted, ScheduledJobPaused, ScheduledJobResumed,
    ScheduledJobUpdated,
};
use crate::scheduled_job::operations::{
    CreateScheduledJobCommand, DeleteScheduledJobCommand, PauseScheduledJobCommand,
    ResumeScheduledJobCommand, UpdateScheduledJobCommand,
};
use crate::scheduled_job::ScheduledJobRepository;
use crate::subscription::entity::{EventTypeBinding, SubscriptionSource, SubscriptionStatus};
use crate::subscription::operations::{
    CreateSubscriptionCommand, DeleteSubscriptionCommand, EventTypeBindingInput,
    PauseSubscriptionCommand, ResumeSubscriptionCommand, SubscriptionCreated, SubscriptionDeleted,
    SubscriptionPaused, SubscriptionResumed, SubscriptionUpdated, UpdateSubscriptionCommand,
};
use crate::usecase::{ExecutionContext, UnitOfWork, UseCaseError};
use crate::{
    ApplicationRepository, DispatchPool, DispatchPoolRepository, Subscription,
    SubscriptionRepository,
};

const KEY_PREFIX: &str = "fn-";

/// 8 hex characters of `sha256(input)` (Java `FunctionTriggerSync.hash8`).
pub fn hash8(input: &str) -> String {
    hex::encode(&Sha256::digest(input.as_bytes())[..4])
}

/// The function id lower-cased without its `fnc_` prefix (Java `fid`):
/// legal in every code pattern, unique by construction.
pub fn fid(function_id: &str) -> String {
    function_id
        .strip_prefix("fnc_")
        .unwrap_or(function_id)
        .to_lowercase()
}

/// A function's pool code and trigger key: `fn-<fid>`.
pub fn pool_key(function_id: &str) -> String {
    format!("{KEY_PREFIX}{}", fid(function_id))
}

/// A subscription's code and trigger key: `fn-<fid>-<hash8(eventType)>`.
pub fn subscription_key(function_id: &str, event_type: &str) -> String {
    keyed(&fid(function_id), hash8, event_type)
}

/// A schedule's code and trigger key: `fn-<fid>-<hash8(cron NUL zone)>`,
/// the zone as written, `""` when absent.
pub fn schedule_key(function_id: &str, cron: &str, timezone: Option<&str>) -> String {
    keyed(
        &fid(function_id),
        hash8,
        &schedule_hash_input(cron, timezone),
    )
}

fn keyed(fid: &str, hasher: fn(&str) -> String, input: &str) -> String {
    format!("{KEY_PREFIX}{fid}-{}", hasher(input))
}

fn schedule_hash_input(cron: &str, timezone: Option<&str>) -> String {
    format!("{cron}\0{}", timezone.unwrap_or(""))
}

fn spec_subscription_key(fid: &str, hasher: fn(&str) -> String, spec: &SubscriptionSpec) -> String {
    keyed(fid, hasher, &spec.event_type)
}

fn spec_schedule_key(fid: &str, hasher: fn(&str) -> String, spec: &ScheduleSpec) -> String {
    keyed(
        fid,
        hasher,
        &schedule_hash_input(&spec.cron, spec.timezone.as_deref()),
    )
}

/// Java `PromoteVersion.missingSettings`: every key the manifest declares
/// (`config`, `secrets`, each `db[].secretRef`) with no value set, first
/// seen first, each once.
pub async fn missing_settings(
    settings: &FunctionSettingsRepository,
    function_id: &str,
    manifest: &Manifest,
) -> Result<Vec<String>, UseCaseError> {
    let (config, secrets) = tokio::try_join!(
        settings.config_map(function_id),
        settings.list_secrets(function_id),
    )?;
    let secret_keys: HashSet<&str> = secrets.iter().map(|s| s.key.as_str()).collect();
    let mut missing: Vec<String> = Vec::new();
    let mut push = |key: &str| {
        if !missing.iter().any(|m| m == key) {
            missing.push(key.to_string());
        }
    };
    for key in &manifest.config {
        if !config.contains_key(key) {
            push(key);
        }
    }
    for key in &manifest.secrets {
        if !secret_keys.contains(key.as_str()) {
            push(key);
        }
    }
    for db in &manifest.db {
        if !secret_keys.contains(db.secret_ref.as_str()) {
            push(&db.secret_ref);
        }
    }
    Ok(missing)
}

#[derive(Clone)]
pub struct TriggerSync {
    pub subscriptions: Arc<SubscriptionRepository>,
    pub pools: Arc<DispatchPoolRepository>,
    pub jobs: Arc<ScheduledJobRepository>,
    pub trigger_objects: Arc<TriggerObjectRepository>,
    pub applications: Arc<ApplicationRepository>,
    pub versions: Arc<FunctionVersionRepository>,
    pub functions: Arc<FunctionRepository>,
    pub routes: Arc<FunctionRouteRepository>,
    pub settings: Arc<FunctionSettingsRepository>,
    /// `FC_FN_POOL_URL`, resolved once at startup.
    pub pool_url: PoolUrlTemplate,
    /// [`hash8`]; a test forces a collision through it (Java's test seam).
    pub hasher: fn(&str) -> String,
}

/// A function's linked objects as they stand: each link with the object it
/// names, `None` when that object was deleted by hand. By trigger key.
struct Current {
    pool: BTreeMap<String, (TriggerObject, Option<DispatchPool>)>,
    subscriptions: BTreeMap<String, (TriggerObject, Option<Subscription>)>,
    jobs: BTreeMap<String, (TriggerObject, Option<ScheduledJob>)>,
}

/// The pool a subscription is compared against: what the pool will be
/// after this promote. `None` when it is still to be created, which no
/// existing subscription can already name.
struct ResolvedPool {
    id: Option<String>,
    code: String,
}

impl TriggerSync {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        subscriptions: Arc<SubscriptionRepository>,
        pools: Arc<DispatchPoolRepository>,
        jobs: Arc<ScheduledJobRepository>,
        trigger_objects: Arc<TriggerObjectRepository>,
        applications: Arc<ApplicationRepository>,
        versions: Arc<FunctionVersionRepository>,
        functions: Arc<FunctionRepository>,
        routes: Arc<FunctionRouteRepository>,
        settings: Arc<FunctionSettingsRepository>,
        pool_url: PoolUrlTemplate,
    ) -> Self {
        Self {
            subscriptions,
            pools,
            jobs,
            trigger_objects,
            applications,
            versions,
            functions,
            routes,
            settings,
            pool_url,
            hasher: hash8,
        }
    }

    /// [`TriggerSync::new`] from the platform's repositories.
    pub fn from_repositories(
        repos: &crate::repository::Repositories,
        settings: Arc<FunctionSettingsRepository>,
        pool_url: PoolUrlTemplate,
    ) -> Self {
        Self::new(
            repos.subscription_repo.clone(),
            repos.dispatch_pool_repo.clone(),
            repos.scheduled_job_repo.clone(),
            repos.function_trigger_object_repo.clone(),
            repos.application_repo.clone(),
            repos.function_version_repo.clone(),
            repos.function_repo.clone(),
            repos.function_route_repo.clone(),
            settings,
            pool_url,
        )
    }

    // ── plan: reads only ────────────────────────────────────────────────────

    /// What promoting `manifest` as version `to_version` to `alias` would
    /// do, from `f`'s current (pre-promote) state. `caller` decides whether
    /// a taken route names the function that holds it.
    pub async fn plan(
        &self,
        f: &Function,
        manifest: &Manifest,
        to_version: i32,
        alias: &str,
        caller: &Caller,
    ) -> Result<PromotePlan, UseCaseError> {
        let from_version = match f.version_id_of(alias) {
            Some(id) => self.versions.find_by_id(id).await?.map(|v| v.version),
            None => None,
        };
        let settings_missing = missing_settings(&self.settings, &f.id, manifest).await?;
        if alias != LIVE_ALIAS {
            return Ok(PromotePlan {
                alias: alias.to_string(),
                from_version,
                to_version,
                settings_missing,
                wiring: Wiring::HttpOnly,
                conflicts: Vec::new(),
            });
        }

        let mut conflicts = Vec::new();
        let fid = fid(&f.id);
        let current = self.current(&f.id).await?;
        let pool_key = pool_key(&f.id);
        let desired_concurrency = manifest.limits.max_concurrency;
        let linked_pool = current.pool.get(&pool_key).and_then(|(_, p)| p.as_ref());
        let pool = match linked_pool {
            None => PoolAction::Create {
                key: pool_key.clone(),
            },
            Some(p) if p.concurrency == desired_concurrency => PoolAction::Unchanged {
                key: pool_key.clone(),
            },
            Some(_) => PoolAction::Update {
                key: pool_key.clone(),
                changed_fields: vec!["maxConcurrency".into()],
            },
        };
        let resolved_pool = ResolvedPool {
            id: linked_pool.map(|p| p.id.clone()),
            code: linked_pool.map_or(pool_key.clone(), |p| p.code.clone()),
        };

        let application_code = self.application_code(f).await?;
        let subscriptions = self.classify_subscriptions(
            f,
            manifest,
            &application_code,
            &resolved_pool,
            &fid,
            &current,
            &mut conflicts,
        );
        let schedules = self.classify_schedules(f, manifest, &fid, &current, &mut conflicts)?;
        let public_routes = self
            .classify_public_routes(f, manifest, caller, &mut conflicts)
            .await?;
        Ok(PromotePlan {
            alias: alias.to_string(),
            from_version,
            to_version,
            settings_missing,
            wiring: Wiring::Live {
                pool,
                subscriptions,
                schedules,
                public_routes,
            },
            conflicts,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn classify_subscriptions(
        &self,
        f: &Function,
        manifest: &Manifest,
        application_code: &str,
        pool: &ResolvedPool,
        fid: &str,
        current: &Current,
        conflicts: &mut Vec<Conflict>,
    ) -> Vec<SubscriptionAction> {
        let keys: Vec<String> = manifest
            .subscriptions
            .iter()
            .map(|s| spec_subscription_key(fid, self.hasher, s))
            .collect();
        collision_conflicts(&keys, "subscriptions", conflicts);

        let mut actions = Vec::new();
        for (key, spec) in keys.iter().zip(&manifest.subscriptions) {
            let existing = current.subscriptions.get(key).and_then(|(_, s)| s.as_ref());
            let Some(existing) = existing else {
                actions.push(SubscriptionAction::Create {
                    trigger_key: key.clone(),
                    event_type: spec.event_type.clone(),
                });
                continue;
            };
            if self.same_subscription(existing, f, manifest, application_code, pool, spec) {
                actions.push(SubscriptionAction::Unchanged {
                    trigger_key: key.clone(),
                    event_type: spec.event_type.clone(),
                });
            } else {
                actions.push(SubscriptionAction::Update {
                    trigger_key: key.clone(),
                    event_type: spec.event_type.clone(),
                    changed_fields: vec!["subscription".into()],
                });
            }
        }
        for (key, (_, object)) in &current.subscriptions {
            if keys.contains(key) {
                continue;
            }
            if let Some(s) = object {
                actions.push(SubscriptionAction::Delete {
                    trigger_key: key.clone(),
                    event_type: s
                        .event_types
                        .first()
                        .map(|b| b.event_type_code.clone())
                        .unwrap_or_default(),
                });
            }
        }
        actions
    }

    /// Everything the manifest controls, the binding's `filter` excluded
    /// (it has no column, so it always reads back empty).
    fn same_subscription(
        &self,
        current: &Subscription,
        f: &Function,
        manifest: &Manifest,
        application_code: &str,
        pool: &ResolvedPool,
        spec: &SubscriptionSpec,
    ) -> bool {
        current.name == subscription_name(f, spec)
            && current.endpoint == self.endpoint_for(manifest, f, spec.path.value())
            && current.application_code.as_deref() == Some(application_code)
            && current.client_id.as_deref() == f.owner.client_id_or_none()
            && pool.id.is_some()
            && current.dispatch_pool_id == pool.id
            && current.dispatch_pool_code.as_deref() == Some(pool.code.as_str())
            && current.mode == dispatch_mode(spec.mode)
            && current.max_retries == spec.max_retries
            && current.timeout_seconds == spec.timeout_seconds
            && current.data_only == spec.data_only
            && current.event_types.len() == 1
            && current.event_types[0].event_type_code == spec.event_type
    }

    fn classify_schedules(
        &self,
        f: &Function,
        manifest: &Manifest,
        fid: &str,
        current: &Current,
        conflicts: &mut Vec<Conflict>,
    ) -> Result<Vec<ScheduleAction>, UseCaseError> {
        let keys: Vec<String> = manifest
            .schedules
            .iter()
            .map(|s| spec_schedule_key(fid, self.hasher, s))
            .collect();
        collision_conflicts(&keys, "schedules", conflicts);

        let mut actions = Vec::new();
        for (key, spec) in keys.iter().zip(&manifest.schedules) {
            let existing = current.jobs.get(key).and_then(|(_, j)| j.as_ref());
            let Some(existing) = existing else {
                actions.push(ScheduleAction::Create {
                    trigger_key: key.clone(),
                    cron: spec.cron.clone(),
                    timezone: spec.timezone.clone(),
                });
                continue;
            };
            let definition = self.job_definition(f, manifest, spec)?;
            // The same decision apply's write makes: a pure copy-or-nothing.
            if existing
                .reconcile(&definition, Some(&f.application_id), None)
                .is_none()
            {
                actions.push(ScheduleAction::Unchanged {
                    trigger_key: key.clone(),
                    cron: spec.cron.clone(),
                    timezone: spec.timezone.clone(),
                });
            } else {
                actions.push(ScheduleAction::Update {
                    trigger_key: key.clone(),
                    cron: spec.cron.clone(),
                    timezone: spec.timezone.clone(),
                    changed_fields: vec!["definition".into()],
                });
            }
        }
        for (key, (_, object)) in &current.jobs {
            if keys.contains(key) {
                continue;
            }
            if let Some(j) = object {
                actions.push(ScheduleAction::Delete {
                    trigger_key: key.clone(),
                    cron: j.crons.first().cloned().unwrap_or_default(),
                    timezone: Some(j.timezone.clone()),
                });
            }
        }
        Ok(actions)
    }

    /// The same `PUBLIC_ROUTE_TAKEN` check publish makes (another function
    /// may have promoted in between), then the set-valued diff against
    /// `fn_routes`.
    async fn classify_public_routes(
        &self,
        f: &Function,
        manifest: &Manifest,
        caller: &Caller,
        conflicts: &mut Vec<Conflict>,
    ) -> Result<PublicRoutesAction, UseCaseError> {
        let wanted: Vec<_> = manifest.public_routes.iter().collect();
        let taken = routes_taken(&self.routes, &self.functions, f, &wanted, caller).await?;
        for e in taken.into_iter().flatten() {
            conflicts.push(Conflict {
                code: e.code().to_string(),
                message: e.message().to_string(),
            });
        }

        let current = self.routes.list_by_function(&f.id).await?;
        let current_keys: Vec<RouteKey> = current
            .iter()
            .map(|r| RouteKey {
                hostname: r.hostname.value().to_string(),
                path_prefix: r.path_prefix.value().to_string(),
                alias_prefixes: r.alias_prefixes.clone(),
            })
            .collect();
        let desired_keys: Vec<RouteKey> = manifest
            .public_routes
            .iter()
            .map(|r| RouteKey {
                hostname: r.hostname.value().to_string(),
                path_prefix: r.path_prefix.value().to_string(),
                alias_prefixes: r.alias_prefixes.clone(),
            })
            .collect();
        let current_set: HashSet<String> = current_keys.iter().map(route_identity).collect();
        let desired_set: HashSet<String> = desired_keys.iter().map(route_identity).collect();
        if current_keys.len() == desired_keys.len() && current_set == desired_set {
            return Ok(PublicRoutesAction::Unchanged);
        }
        Ok(PublicRoutesAction::Replace {
            added: desired_keys
                .iter()
                .filter(|k| !current_set.contains(&route_identity(k)))
                .cloned()
                .collect(),
            removed: current_keys
                .iter()
                .filter(|k| !desired_set.contains(&route_identity(k)))
                .cloned()
                .collect(),
        })
    }

    /// The `fn_routes` rows promoting `manifest` to `live` writes, or `None`
    /// when the plan leaves them as they are (no difference, no write) or
    /// the alias is a named one.
    pub fn routes_for(
        &self,
        f: &Function,
        manifest: &Manifest,
        plan: &PromotePlan,
        now: DateTime<Utc>,
    ) -> Option<Vec<FunctionRoute>> {
        match &plan.wiring {
            Wiring::Live {
                public_routes: PublicRoutesAction::Replace { .. },
                ..
            } => Some(
                manifest
                    .public_routes
                    .iter()
                    .map(|r| {
                        FunctionRoute::of(
                            &f.id,
                            r.hostname.clone(),
                            r.path_prefix.clone(),
                            r.alias_prefixes.clone(),
                            now,
                        )
                    })
                    .collect(),
            ),
            _ => None,
        }
    }

    // ── apply: exactly a plan ───────────────────────────────────────────────

    /// Performs `plan` (made from `new_live`'s manifest) on `uow`. A plan
    /// with a conflict writes nothing and fails with the first; a named
    /// alias's plan is a no-op. The public routes are the promote's own
    /// commit, not this.
    pub async fn apply<U: UnitOfWork>(
        &self,
        uow: &U,
        f: &Function,
        new_live: &FunctionVersion,
        ctx: &ExecutionContext,
        plan: &PromotePlan,
    ) -> Result<(), UseCaseError> {
        if let Some(first) = plan.conflicts.first() {
            return Err(first.to_error());
        }
        let Wiring::Live {
            pool,
            subscriptions,
            schedules,
            ..
        } = &plan.wiring
        else {
            return Ok(());
        };
        let manifest = &new_live.manifest;
        let now = Utc::now();
        let fid = fid(&f.id);
        let application_code = self.application_code(f).await?;
        let current = self.current(&f.id).await?;

        let dispatch_pool = self
            .apply_pool(uow, ctx, f, manifest, pool, &current, now)
            .await?;

        let specs: HashMap<String, &SubscriptionSpec> = manifest
            .subscriptions
            .iter()
            .map(|s| (spec_subscription_key(&fid, self.hasher, s), s))
            .collect();
        let schedule_specs: HashMap<String, &ScheduleSpec> = manifest
            .schedules
            .iter()
            .map(|s| (spec_schedule_key(&fid, self.hasher, s), s))
            .collect();

        let mut delete_subscriptions = Vec::new();
        for action in subscriptions {
            let key = action.trigger_key();
            match action {
                SubscriptionAction::Create { .. } | SubscriptionAction::Update { .. } => {
                    let spec = specs.get(key).ok_or_else(|| plan_mismatch(key))?;
                    self.apply_subscription(
                        uow,
                        ctx,
                        f,
                        manifest,
                        &application_code,
                        &dispatch_pool,
                        key,
                        spec,
                        &current,
                        now,
                    )
                    .await?;
                }
                SubscriptionAction::Delete { .. } => delete_subscriptions.push(key),
                SubscriptionAction::Unchanged { .. } => {}
            }
        }
        let mut delete_jobs = Vec::new();
        for action in schedules {
            let key = action.trigger_key();
            match action {
                ScheduleAction::Create { .. } | ScheduleAction::Update { .. } => {
                    let spec = schedule_specs.get(key).ok_or_else(|| plan_mismatch(key))?;
                    self.apply_schedule(uow, ctx, f, manifest, key, spec, &current, now)
                        .await?;
                }
                ScheduleAction::Delete { .. } => delete_jobs.push(key),
                ScheduleAction::Unchanged { .. } => {}
            }
        }

        // Deletions last: subscriptions, then schedules. One deleted by hand
        // since the plan has nothing left to delete.
        for key in delete_subscriptions {
            if let Some((link, Some(s))) = current.subscriptions.get(key) {
                self.delete_subscription(uow, ctx, link, s).await?;
            }
        }
        for key in delete_jobs {
            if let Some((link, Some(j))) = current.jobs.get(key) {
                self.delete_job(uow, ctx, link, j).await?;
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn apply_pool<U: UnitOfWork>(
        &self,
        uow: &U,
        ctx: &ExecutionContext,
        f: &Function,
        manifest: &Manifest,
        action: &PoolAction,
        current: &Current,
        now: DateTime<Utc>,
    ) -> Result<DispatchPool, UseCaseError> {
        let desired = manifest.limits.max_concurrency;
        let linked = |key: &str| {
            current
                .pool
                .get(key)
                .and_then(|(_, p)| p.clone())
                .ok_or_else(|| plan_mismatch(key))
        };
        match action {
            // No difference: no write, no event.
            PoolAction::Unchanged { key } => linked(key),
            PoolAction::Update { key, .. } => {
                let mut pool = linked(key)?;
                pool.concurrency = desired;
                pool.updated_at = now;
                let event = DispatchPoolUpdated::new(ctx, &pool.id, &pool.name);
                let command = UpdateDispatchPoolCommand {
                    id: pool.id.clone(),
                    name: None,
                    description: None,
                    rate_limit: None,
                    concurrency: Some(desired as u32),
                };
                self.commit_linked(
                    uow,
                    f,
                    TriggerObjectKind::Pool,
                    key,
                    pool,
                    &*self.pools,
                    event,
                    &command,
                    now,
                )
                .await
            }
            PoolAction::Create { key } => {
                let name = format!("function {}", f.address.render());
                let pool =
                    DispatchPool::new(key.clone(), name.clone()).with_concurrency(desired as u32);
                let event = DispatchPoolCreated::new(ctx, &pool.id, &pool.code, &pool.name);
                let command = CreateDispatchPoolCommand {
                    code: key.clone(),
                    name,
                    description: None,
                    client_id: None,
                    rate_limit: None,
                    concurrency: Some(desired as u32),
                };
                self.commit_linked(
                    uow,
                    f,
                    TriggerObjectKind::Pool,
                    key,
                    pool,
                    &*self.pools,
                    event,
                    &command,
                    now,
                )
                .await
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn apply_subscription<U: UnitOfWork>(
        &self,
        uow: &U,
        ctx: &ExecutionContext,
        f: &Function,
        manifest: &Manifest,
        application_code: &str,
        pool: &DispatchPool,
        key: &str,
        spec: &SubscriptionSpec,
        current: &Current,
        now: DateTime<Utc>,
    ) -> Result<(), UseCaseError> {
        let endpoint = self.endpoint_for(manifest, f, spec.path.value());
        let name = subscription_name(f, spec);
        let client_id = f.owner.client_id_or_none().map(str::to_string);
        // No manifest filter: a binding's filter has no column.
        let binding = EventTypeBinding::new(spec.event_type.clone());
        let binding_input = vec![EventTypeBindingInput {
            event_type_code: spec.event_type.clone(),
            filter: None,
            event_type_id: None,
            spec_version: None,
        }];

        match current.subscriptions.get(key).and_then(|(_, s)| s.clone()) {
            Some(mut sub) => {
                sub.name = name.clone();
                sub.application_code = Some(application_code.to_string());
                sub.client_id = client_id;
                sub.endpoint = endpoint.clone();
                sub.dispatch_pool_id = Some(pool.id.clone());
                sub.dispatch_pool_code = Some(pool.code.clone());
                sub.event_types = vec![binding];
                sub.mode = dispatch_mode(spec.mode);
                sub.timeout_seconds = spec.timeout_seconds;
                sub.max_retries = spec.max_retries;
                sub.data_only = spec.data_only;
                sub.updated_at = now;
                let event = SubscriptionUpdated::new(ctx, &sub.id, &name);
                let command = UpdateSubscriptionCommand {
                    subscription_id: sub.id.clone(),
                    name: Some(name),
                    description: None,
                    endpoint: Some(endpoint),
                    connection_id: None,
                    event_types: Some(binding_input),
                    dispatch_pool_id: Some(pool.id.clone()),
                    service_account_id: None,
                    mode: Some(dispatch_mode(spec.mode)),
                    max_retries: Some(spec.max_retries as u32),
                    timeout_seconds: Some(spec.timeout_seconds as u32),
                    data_only: Some(spec.data_only),
                    queue: None,
                    delay_seconds: None,
                    max_age_seconds: None,
                    custom_config: None,
                    // Platform-authored: a function subscription names no
                    // account or connection, so no signer is checked.
                    caller: None,
                };
                self.commit_linked(
                    uow,
                    f,
                    TriggerObjectKind::Subscription,
                    key,
                    sub,
                    &*self.subscriptions,
                    event,
                    &command,
                    now,
                )
                .await?;
            }
            None => {
                let mut sub = Subscription::new(key, name.clone(), endpoint.clone());
                sub.application_code = Some(application_code.to_string());
                sub.client_id = client_id.clone();
                sub.event_types = vec![binding];
                sub.source = SubscriptionSource::Function;
                sub.dispatch_pool_id = Some(pool.id.clone());
                sub.dispatch_pool_code = Some(pool.code.clone());
                sub.mode = dispatch_mode(spec.mode);
                sub.max_retries = spec.max_retries;
                sub.timeout_seconds = spec.timeout_seconds;
                sub.data_only = spec.data_only;
                let event = SubscriptionCreated::new(ctx, &sub.id, &sub.code, &sub.name);
                let command = CreateSubscriptionCommand {
                    code: key.to_string(),
                    name,
                    description: None,
                    client_id,
                    endpoint,
                    connection_id: None,
                    event_types: binding_input,
                    dispatch_pool_id: Some(pool.id.clone()),
                    service_account_id: None,
                    mode: Some(dispatch_mode(spec.mode)),
                    max_retries: Some(spec.max_retries as u32),
                    timeout_seconds: Some(spec.timeout_seconds as u32),
                    data_only: spec.data_only,
                    queue: None,
                    delay_seconds: None,
                    max_age_seconds: None,
                    custom_config: None,
                    caller: None,
                };
                self.commit_linked(
                    uow,
                    f,
                    TriggerObjectKind::Subscription,
                    key,
                    sub,
                    &*self.subscriptions,
                    event,
                    &command,
                    now,
                )
                .await?;
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn apply_schedule<U: UnitOfWork>(
        &self,
        uow: &U,
        ctx: &ExecutionContext,
        f: &Function,
        manifest: &Manifest,
        key: &str,
        spec: &ScheduleSpec,
        current: &Current,
        now: DateTime<Utc>,
    ) -> Result<(), UseCaseError> {
        let d = self.job_definition(f, manifest, spec)?;
        match current.jobs.get(key).and_then(|(_, j)| j.as_ref()) {
            Some(existing) => {
                let Some((job, _changed)) =
                    existing.reconcile(&d, Some(&f.application_id), Some(&ctx.principal_id))
                else {
                    return Ok(()); // no difference: no write, no event
                };
                let event = ScheduledJobUpdated::new(ctx, &job.id, &job.code);
                let command = UpdateScheduledJobCommand {
                    scheduled_job_id: job.id.clone(),
                    name: Some(d.name.clone()),
                    description: None,
                    crons: Some(d.crons.clone()),
                    timezone: Some(d.timezone.clone()),
                    payload: d.payload.clone(),
                    concurrent: Some(false),
                    tracks_completion: Some(false),
                    timeout_seconds: None,
                    delivery_max_attempts: None,
                    target_url: d.target_url.clone(),
                };
                self.commit_linked(
                    uow,
                    f,
                    TriggerObjectKind::ScheduledJob,
                    key,
                    job,
                    &*self.jobs,
                    event,
                    &command,
                    now,
                )
                .await?;
            }
            None => {
                // Scoped to the function's owner: no client for a platform
                // function, never a platform-wide default.
                let client_id = f.owner.client_id_or_none().map(str::to_string);
                let mut job = ScheduledJob::new(key, d.name.clone(), d.crons.clone())
                    .with_timezone(d.timezone.clone())
                    .with_application_id(f.application_id.clone())
                    .with_created_by(ctx.principal_id.clone());
                job.client_id = client_id.clone();
                job.payload = d.payload.clone();
                job.target_url = d.target_url.clone();
                let event = ScheduledJobCreated::new(ctx, &job.id, &job.code);
                let command = CreateScheduledJobCommand {
                    code: key.to_string(),
                    name: d.name.clone(),
                    description: None,
                    client_id,
                    crons: d.crons.clone(),
                    timezone: d.timezone.clone(),
                    payload: d.payload.clone(),
                    concurrent: false,
                    tracks_completion: false,
                    timeout_seconds: None,
                    delivery_max_attempts: d.delivery_max_attempts,
                    target_url: d.target_url.clone(),
                };
                self.commit_linked(
                    uow,
                    f,
                    TriggerObjectKind::ScheduledJob,
                    key,
                    job,
                    &*self.jobs,
                    event,
                    &command,
                    now,
                )
                .await?;
            }
        }
        Ok(())
    }

    /// What a schedule entry makes its job (Java's `ScheduledJob.Definition`
    /// for a function): the cron as Java stores it (its text, stripped; the
    /// scheduler reads Java's dialect), the zone as written or `UTC`, the
    /// payload, the function's URL for the entry's path, not concurrent, no
    /// completion tracking.
    fn job_definition(
        &self,
        f: &Function,
        manifest: &Manifest,
        spec: &ScheduleSpec,
    ) -> Result<JobDefinition, UseCaseError> {
        let crons = vec![
            parse_java_cron(&spec.cron)
                .map_err(|(_, why)| {
                    UseCaseError::validation(
                        "CRON_INVALID",
                        format!("cron expression '{}' invalid: {why}", spec.cron),
                    )
                })?
                .expression,
        ];
        let payload = match &spec.payload {
            None => None,
            Some(node) if node.is_null() => None,
            Some(node) => Some(
                serde_json::to_value(node)
                    .map_err(|e| UseCaseError::internal("PAYLOAD_INVALID", e.to_string()))?,
            ),
        };
        Ok(JobDefinition {
            name: format!("{}: {}", f.address.render(), spec.cron),
            description: None,
            crons,
            timezone: spec
                .timezone
                .clone()
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| "UTC".to_string()),
            payload,
            concurrent: false,
            tracks_completion: false,
            timeout_seconds: None,
            delivery_max_attempts: 3,
            target_url: Some(self.endpoint_for(manifest, f, spec.path.value())),
        })
    }

    // ── delete and status change ────────────────────────────────────────────

    /// Deletes every object `f` owns, each through its own delete and event:
    /// subscriptions, then scheduled jobs, then the pool. `fn_routes` and
    /// the links cascade with the function row itself.
    pub async fn on_delete<U: UnitOfWork>(
        &self,
        uow: &U,
        f: &Function,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        let current = self.current(&f.id).await?;
        for (link, sub) in current.subscriptions.values() {
            if let Some(s) = sub {
                self.delete_subscription(uow, ctx, link, s).await?;
            }
        }
        for (link, job) in current.jobs.values() {
            if let Some(j) = job {
                self.delete_job(uow, ctx, link, j).await?;
            }
        }
        for (link, pool) in current.pool.values() {
            if let Some(p) = pool {
                let event = DispatchPoolDeleted::new(ctx, &p.id, &p.code);
                let command = DeleteDispatchPoolCommand { id: p.id.clone() };
                self.commit_delete_linked(uow, link, p.clone(), &*self.pools, event, &command)
                    .await?;
            }
        }
        Ok(())
    }

    /// After a real status transition of `f`: `DISABLED` pauses each
    /// `ACTIVE` linked subscription and job, `ACTIVE` resumes each `PAUSED`
    /// one. Anything already in the target state is left alone: no write, no
    /// event. The pool never changes.
    pub async fn on_status_change<U: UnitOfWork>(
        &self,
        uow: &U,
        f: &Function,
        ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        let disable = f.status == FunctionStatus::Disabled;
        let current = self.current(&f.id).await?;
        // Java walks the links in (kind, key) order: jobs before subscriptions.
        for (_, job) in current.jobs.values() {
            let Some(mut j) = job.clone() else { continue };
            if disable && j.status == ScheduledJobStatus::Active {
                j.pause();
                j.updated_by = Some(ctx.principal_id.clone());
                let event = ScheduledJobPaused::new(ctx, &j.id, &j.code);
                let command = PauseScheduledJobCommand {
                    scheduled_job_id: j.id.clone(),
                };
                uow.commit(&j, &*self.jobs, event, &command)
                    .await
                    .into_result()?;
            } else if !disable && j.status == ScheduledJobStatus::Paused {
                j.resume();
                j.updated_by = Some(ctx.principal_id.clone());
                let event = ScheduledJobResumed::new(ctx, &j.id, &j.code);
                let command = ResumeScheduledJobCommand {
                    scheduled_job_id: j.id.clone(),
                };
                uow.commit(&j, &*self.jobs, event, &command)
                    .await
                    .into_result()?;
            }
        }
        for (_, sub) in current.subscriptions.values() {
            let Some(mut s) = sub.clone() else { continue };
            if disable && s.status == SubscriptionStatus::Active {
                s.pause();
                let event = SubscriptionPaused::new(ctx, &s.id);
                let command = PauseSubscriptionCommand {
                    subscription_id: s.id.clone(),
                };
                uow.commit(&s, &*self.subscriptions, event, &command)
                    .await
                    .into_result()?;
            } else if !disable && s.status == SubscriptionStatus::Paused {
                s.resume();
                let event = SubscriptionResumed::new(ctx, &s.id);
                let command = ResumeSubscriptionCommand {
                    subscription_id: s.id.clone(),
                };
                uow.commit(&s, &*self.subscriptions, event, &command)
                    .await
                    .into_result()?;
            }
        }
        Ok(())
    }

    async fn delete_subscription<U: UnitOfWork>(
        &self,
        uow: &U,
        ctx: &ExecutionContext,
        link: &TriggerObject,
        s: &Subscription,
    ) -> Result<(), UseCaseError> {
        let event = SubscriptionDeleted::new(ctx, &s.id, &s.code);
        let command = DeleteSubscriptionCommand {
            subscription_id: s.id.clone(),
        };
        self.commit_delete_linked(uow, link, s.clone(), &*self.subscriptions, event, &command)
            .await
    }

    async fn delete_job<U: UnitOfWork>(
        &self,
        uow: &U,
        ctx: &ExecutionContext,
        link: &TriggerObject,
        j: &ScheduledJob,
    ) -> Result<(), UseCaseError> {
        let event = ScheduledJobDeleted::new(ctx, &j.id, &j.code);
        let command = DeleteScheduledJobCommand {
            scheduled_job_id: j.id.clone(),
        };
        self.commit_delete_linked(uow, link, j.clone(), &*self.jobs, event, &command)
            .await
    }

    // ── shared ──────────────────────────────────────────────────────────────

    /// Commits `object` through its own repository with its own event and
    /// command, linked to `f` under `key`; returns the object.
    #[allow(clippy::too_many_arguments)]
    async fn commit_linked<U, A, R, E, C>(
        &self,
        uow: &U,
        f: &Function,
        kind: TriggerObjectKind,
        key: &str,
        object: A,
        repository: &R,
        event: E,
        command: &C,
        now: DateTime<Utc>,
    ) -> Result<A, UseCaseError>
    where
        U: UnitOfWork,
        A: crate::usecase::HasId + Send + Sync,
        R: crate::usecase::Persist<A>,
        E: crate::usecase::DomainEvent + Send + 'static,
        C: serde::Serialize + crate::usecase::AuditMasked + Send + Sync,
    {
        let link = TriggerObject {
            function_id: f.id.clone(),
            kind,
            object_id: object.id().to_string(),
            trigger_key: key.to_string(),
            created_at: now,
        };
        let linked = Linked { object, link };
        let links = LinkedRepository {
            objects: repository,
            links: &self.trigger_objects,
        };
        uow.commit(&linked, &links, event, command)
            .await
            .into_result()?;
        Ok(linked.object)
    }

    async fn commit_delete_linked<U, A, R, E, C>(
        &self,
        uow: &U,
        link: &TriggerObject,
        object: A,
        repository: &R,
        event: E,
        command: &C,
    ) -> Result<(), UseCaseError>
    where
        U: UnitOfWork,
        A: crate::usecase::HasId + Send + Sync,
        R: crate::usecase::Persist<A>,
        E: crate::usecase::DomainEvent + Send + 'static,
        C: serde::Serialize + crate::usecase::AuditMasked + Send + Sync,
    {
        let linked = Linked {
            object,
            link: link.clone(),
        };
        let links = LinkedRepository {
            objects: repository,
            links: &self.trigger_objects,
        };
        uow.commit_delete(&linked, &links, event, command)
            .await
            .into_result()?;
        Ok(())
    }

    /// The links and the objects they name: four queries however many.
    async fn current(&self, function_id: &str) -> Result<Current, UseCaseError> {
        let links = self.trigger_objects.list(function_id).await?;
        let ids = |kind: TriggerObjectKind| -> Vec<String> {
            links
                .iter()
                .filter(|l| l.kind == kind)
                .map(|l| l.object_id.clone())
                .collect()
        };
        let (pool_ids, sub_ids, job_ids) = (
            ids(TriggerObjectKind::Pool),
            ids(TriggerObjectKind::Subscription),
            ids(TriggerObjectKind::ScheduledJob),
        );
        let (pools, subs, jobs) = tokio::try_join!(
            self.pools.find_by_ids(&pool_ids),
            self.subscriptions.find_by_ids(&sub_ids),
            self.jobs.find_by_ids(&job_ids),
        )?;
        let mut pools: HashMap<String, DispatchPool> =
            pools.into_iter().map(|p| (p.id.clone(), p)).collect();
        let mut subs: HashMap<String, Subscription> =
            subs.into_iter().map(|s| (s.id.clone(), s)).collect();
        let mut jobs: HashMap<String, ScheduledJob> =
            jobs.into_iter().map(|j| (j.id.clone(), j)).collect();
        let mut current = Current {
            pool: BTreeMap::new(),
            subscriptions: BTreeMap::new(),
            jobs: BTreeMap::new(),
        };
        for link in links {
            let key = link.trigger_key.clone();
            match link.kind {
                TriggerObjectKind::Pool => {
                    let object = pools.remove(&link.object_id);
                    current.pool.insert(key, (link, object));
                }
                TriggerObjectKind::Subscription => {
                    let object = subs.remove(&link.object_id);
                    current.subscriptions.insert(key, (link, object));
                }
                TriggerObjectKind::ScheduledJob => {
                    let object = jobs.remove(&link.object_id);
                    current.jobs.insert(key, (link, object));
                }
            }
        }
        Ok(current)
    }

    /// The owning application's code; its row must exist.
    async fn application_code(&self, f: &Function) -> Result<String, UseCaseError> {
        match self.applications.find_by_id(&f.application_id).await? {
            Some(app) => Ok(app.code),
            None => Err(UseCaseError::internal(
                "APPLICATION_NOT_FOUND",
                format!(
                    "function '{}' names an application row that is missing",
                    f.address.render()
                ),
            )),
        }
    }

    /// `<pool URL>/functions/<address><literal path>`, never a version: a
    /// dispatch job keeps its target URL, so a version in it would keep
    /// calling the old version after every promote.
    fn endpoint_for(&self, manifest: &Manifest, f: &Function, literal_path: &str) -> String {
        format!(
            "{}/functions/{}{literal_path}",
            self.pool_url.resolve(&manifest.pool),
            f.address.render()
        )
    }
}

fn subscription_name(f: &Function, spec: &SubscriptionSpec) -> String {
    format!("{}: {}", f.address.render(), spec.event_type)
}

/// Two manifest entries whose keys collide are an internal error at
/// promote, never a silent overwrite of one link by the other.
fn collision_conflicts(keys: &[String], what: &str, conflicts: &mut Vec<Conflict>) {
    let mut seen = HashSet::new();
    for key in keys {
        if !seen.insert(key) {
            conflicts.push(Conflict {
                code: "TRIGGER_KEY_COLLISION".into(),
                message: format!(
                    "two {what} entries of this function's manifest hash to the same trigger key '{key}'"
                ),
            });
        }
    }
}

/// `hostname|pathPrefix|sorted alias prefixes`: the alias prefixes are part
/// of a route's identity.
fn route_identity(k: &RouteKey) -> String {
    let mut prefixes = k.alias_prefixes.clone();
    prefixes.sort();
    format!("{}|{}|{}", k.hostname, k.path_prefix, prefixes.join(","))
}

/// The objects changed between the plan and its apply, in one transaction:
/// not something a caller can cause.
fn plan_mismatch(key: &str) -> UseCaseError {
    UseCaseError::internal(
        "PLAN_STALE",
        format!("trigger '{key}' changed between the promote plan and its apply"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_follow_java() {
        assert_eq!(fid("fnc_0HZXEQ5Y8JY5Z"), "0hzxeq5y8jy5z");
        assert_eq!(hash8(""), "e3b0c442");
        assert_eq!(hash8("abc"), "ba7816bf");
    }

    #[test]
    fn a_collision_is_a_conflict_per_repeat() {
        let mut conflicts = Vec::new();
        let keys = vec![
            "fn-a-1".to_string(),
            "fn-a-1".to_string(),
            "fn-a-2".to_string(),
        ];
        collision_conflicts(&keys, "subscriptions", &mut conflicts);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].code, "TRIGGER_KEY_COLLISION");
        assert_eq!(
            conflicts[0].message,
            "two subscriptions entries of this function's manifest hash to the same trigger key 'fn-a-1'"
        );
        let e = conflicts[0].to_error();
        assert_eq!(e.http_status_code(), 500);
    }

    #[test]
    fn route_identity_includes_sorted_alias_prefixes() {
        let k = |p: &[&str]| RouteKey {
            hostname: "a.com".into(),
            path_prefix: "/".into(),
            alias_prefixes: p.iter().map(|s| s.to_string()).collect(),
        };
        assert_eq!(
            route_identity(&k(&["qa", "dev"])),
            route_identity(&k(&["dev", "qa"]))
        );
        assert_ne!(route_identity(&k(&["qa"])), route_identity(&k(&[])));
    }
}
