//! What a publish checks beyond the manifest itself (Java
//! `FunctionTriggerSync.checkPublish`, `FunctionTriggerSync.java:154-310`):
//! the manifest's references to the rest of the platform. Reads only.
//!
//! Every problem is collected, in Java's order: each subscription's event
//! type, each schedule's cron then timezone, the application's signing
//! secret (only when there is a subscription or schedule), warm capacity
//! (only for a warm manifest), then each public route's hostname claim and
//! route conflict. Publish rejects with the first; the manifest check route
//! returns them all.
//!
//! In Java these live on `TriggerSync`, the promote wiring seam; they need
//! none of the wiring, so they stay apart from [`super::TriggerSync`],
//! which shares [`routes_taken`] with them.

use std::collections::HashMap;
use std::sync::Arc;

use super::access::Caller;
use crate::event_type::entity::EventTypeStatus;
use crate::event_type::repository::EventTypeRepository;
use crate::function::domain_repository::FunctionDomainRepository;
use crate::function::entity::{Function, FunctionRoute};
use crate::function::repository::FunctionRepository;
use crate::function::route_repository::FunctionRouteRepository;
use crate::function::schedule_check::{parse_cron, zone_id_valid};
use crate::function::version_repository::FunctionVersionRepository;
use crate::function::{FunctionLimits, Manifest, PublicRoute};
use crate::service_account::repository::ServiceAccountRepository;
use crate::usecase::UseCaseError;

#[derive(Clone)]
pub struct PublishChecks {
    pub event_types: Arc<EventTypeRepository>,
    pub service_accounts: Arc<ServiceAccountRepository>,
    pub versions: Arc<FunctionVersionRepository>,
    pub functions: Arc<FunctionRepository>,
    pub domains: Arc<FunctionDomainRepository>,
    pub routes: Arc<FunctionRouteRepository>,
    pub limits: FunctionLimits,
}

impl PublishChecks {
    /// Every problem publishing `manifest` for `f` would meet, in Java's
    /// order; empty when there is none. `caller` decides whether a taken
    /// route names the function that holds it.
    pub async fn check(
        &self,
        f: &Function,
        manifest: &Manifest,
        caller: &Caller,
    ) -> Result<Vec<UseCaseError>, UseCaseError> {
        let mut errors = Vec::new();

        let codes: Vec<String> = manifest
            .subscriptions
            .iter()
            .map(|s| s.event_type.clone())
            .collect();
        let statuses = self.event_types.statuses_by_codes(&codes).await?;
        for code in &codes {
            match statuses.get(code) {
                None => errors.push(UseCaseError::validation(
                    "EVENT_TYPE_NOT_FOUND",
                    format!("event type '{code}' not found"),
                )),
                Some(EventTypeStatus::Archived) => errors.push(UseCaseError::validation(
                    "EVENT_TYPE_NOT_FOUND",
                    format!("event type '{code}' is archived"),
                )),
                Some(_) => {}
            }
        }

        for schedule in &manifest.schedules {
            if let Err((_, why)) = parse_cron(&schedule.cron) {
                errors.push(UseCaseError::validation(
                    "CRON_INVALID",
                    format!("cron expression '{}' invalid: {why}", schedule.cron),
                ));
            }
            if let Some(zone) = &schedule.timezone {
                if !zone_id_valid(zone) {
                    errors.push(UseCaseError::validation(
                        "TIMEZONE_INVALID",
                        format!("timezone '{zone}' is not a recognised IANA zone"),
                    ));
                }
            }
        }

        if (!manifest.subscriptions.is_empty() || !manifest.schedules.is_empty())
            && !self
                .service_accounts
                .oldest_active_has_signing_secret(&f.application_id)
                .await?
        {
            errors.push(UseCaseError::validation(
                "APPLICATION_SIGNING_SECRET_REQUIRED",
                "the function's application needs an active service account with a signing \
                 secret before it can declare subscriptions or schedules",
            ));
        }

        if manifest.warm {
            let live_warm = self
                .versions
                .count_live_warm_in_pool(&manifest.pool, &f.id)
                .await?;
            if live_warm + 1 > self.limits.max_warm_per_host() {
                errors.push(UseCaseError::validation(
                    "WARM_CAPACITY_EXCEEDED",
                    format!(
                        "pool '{}' is at its warm-function limit ({})",
                        manifest.pool.value(),
                        self.limits.max_warm_per_host()
                    ),
                ));
            }
        }

        errors.extend(self.check_public_routes(f, manifest, caller).await?);
        Ok(errors)
    }

    /// Each `public[]` entry's hostname must be under a domain claimed by
    /// this function's owner (`PUBLIC_HOSTNAME_NOT_CLAIMED`, the same answer
    /// for unclaimed and claimed-by-another, so it is no oracle), and its
    /// `(hostname, pathPrefix)` must not already route to another function
    /// (`PUBLIC_ROUTE_TAKEN`). An unclaimed entry skips its own route check;
    /// every other entry is still checked.
    async fn check_public_routes(
        &self,
        f: &Function,
        manifest: &Manifest,
        caller: &Caller,
    ) -> Result<Vec<UseCaseError>, UseCaseError> {
        let routes = &manifest.public_routes;
        if routes.is_empty() {
            return Ok(Vec::new());
        }
        let hostnames: Vec<_> = routes.iter().map(|r| &r.hostname).collect();
        let wanted: Vec<_> = routes.iter().collect();
        let (claims, taken) = tokio::try_join!(
            async { Ok::<_, UseCaseError>(self.domains.covering_each(&hostnames).await?) },
            routes_taken(&self.routes, &self.functions, f, &wanted, caller),
        )?;

        let mut errors = Vec::new();
        for ((route, claim), taken) in routes.iter().zip(claims).zip(taken) {
            if claim.is_none_or(|d| d.owner != f.owner) {
                errors.push(UseCaseError::validation(
                    "PUBLIC_HOSTNAME_NOT_CLAIMED",
                    format!(
                        "hostname '{}' is not under a domain claimed by this function's owner",
                        route.hostname.value()
                    ),
                ));
                continue;
            }
            errors.extend(taken);
        }
        Ok(errors)
    }
}

/// For each of `wanted`, the `409 PUBLIC_ROUTE_TAKEN` it meets when another
/// function already holds its `(hostname, pathPrefix)` (Java
/// `FunctionTriggerSync.publicRouteTakenError`): naming that function only
/// when `caller` could reach it anyway. Publish and the promote plan share
/// it. Two queries however many routes.
pub(crate) async fn routes_taken(
    routes: &FunctionRouteRepository,
    functions: &FunctionRepository,
    f: &Function,
    wanted: &[&PublicRoute],
    caller: &Caller,
) -> Result<Vec<Option<UseCaseError>>, UseCaseError> {
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let keys: Vec<_> = wanted
        .iter()
        .map(|r| (&r.hostname, &r.path_prefix))
        .collect();
    let taken = routes.find_public_each(&keys).await?;
    let taken: Vec<&FunctionRoute> = taken.iter().filter(|r| r.function_id != f.id).collect();
    let mut holder_ids: Vec<String> = taken.iter().map(|r| r.function_id.clone()).collect();
    holder_ids.sort();
    holder_ids.dedup();
    let holders: HashMap<String, Function> = functions
        .find_by_ids(&holder_ids)
        .await?
        .into_iter()
        .map(|h| (h.id.clone(), h))
        .collect();
    Ok(wanted
        .iter()
        .map(|route| {
            let existing = taken.iter().find(|r| {
                r.hostname == route.hostname && r.path_prefix.value() == route.path_prefix.value()
            })?;
            let text = format!("{}{}", route.hostname.value(), route.path_prefix.value());
            let message = match holders.get(&existing.function_id) {
                Some(other) if caller.can_reach(other) => format!(
                    "route '{text}' is already taken by function '{}'",
                    other.address.render()
                ),
                _ => format!("route '{text}' is already taken"),
            };
            Some(UseCaseError::business_rule("PUBLIC_ROUTE_TAKEN", message))
        })
        .collect())
}
