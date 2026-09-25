//! Whether a caller may cause the signed delivery of what it ingests (Java
//! `dispatchjob/processing/DeliverySigningGuard.java`, b1ce6e55, eac7ef57;
//! `docs/parity/java-2026-09-25-triage.md` S5).
//!
//! An ingested dispatch job carries a payload and target of the caller's
//! choosing, and is delivered with whichever identity
//! [`signer_of`](super::delivery_credentials::signer_of) picks: its
//! subscription's account, that subscription's connection's account, or the
//! application's own account (the subscription's application, else the job
//! code's leading segment). Without this check, ingest is a signing oracle
//! for any account the resolver can reach. So, before anything is written:
//!
//! 1. a non-anchor caller's job may name only a subscription of the job's
//!    own client (a subscription id naming no row is ignored, exactly as the
//!    resolver ignores it);
//! 2. the identity that would sign must be one the caller may use
//!    ([`SigningReach`]): a named account through `may_use` (no
//!    owning-application exemption: the payload and target are the
//!    caller's, so a subscription's ownership proves nothing about them); an
//!    application's own account through `may_use_application`. A named
//!    account or application that does not exist signs nothing, so it is
//!    not refused.
//!
//! An ingested **event** of application X's type fans out to X's
//! subscriptions, and one with no account or connection of its own is signed
//! with X's account, so ingesting it causes X's signature on a payload the
//! caller chose (Java `DeliverySigningGuard.checkEvent`, d0f7eb20; owner
//! ruling 17a). It is accepted only from X's own account, a super-admin, or
//! an anchor whose application access covers X (the operator's staff). A
//! type naming no known application signs nothing and passes.
//!
//! Every lookup is batched: a batch of a thousand jobs costs a fixed handful
//! of queries.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use super::delivery_credentials::{connection_to_load, leading_segment, signer_of, Signer};
use super::entity::DispatchJob;
use crate::connection::entity::Connection;
use crate::connection::repository::ConnectionRepository;
use crate::service_account::signing_reach::SigningReach;
use crate::shared::authorization_service::{ApplicationScope, AuthContext};
use crate::shared::error::{PlatformError, Result};
use crate::PrincipalRepository;
use crate::{
    ApplicationRepository, ServiceAccountRepository, Subscription, SubscriptionRepository,
};

pub struct SigningGuard {
    subscriptions: Arc<SubscriptionRepository>,
    connections: Arc<ConnectionRepository>,
    accounts: Arc<ServiceAccountRepository>,
    applications: Arc<ApplicationRepository>,
    principals: Arc<PrincipalRepository>,
}

impl SigningGuard {
    pub fn new(
        subscriptions: Arc<SubscriptionRepository>,
        connections: Arc<ConnectionRepository>,
        accounts: Arc<ServiceAccountRepository>,
        applications: Arc<ApplicationRepository>,
        principals: Arc<PrincipalRepository>,
    ) -> Self {
        Self {
            subscriptions,
            connections,
            accounts,
            applications,
            principals,
        }
    }

    /// `Ok` when `caller` may ingest events of every one of `event_types`;
    /// otherwise a 403 for the whole request, naming the first type refused.
    pub async fn check_event_types<'a>(
        &self,
        caller: &AuthContext,
        event_types: impl IntoIterator<Item = &'a str>,
    ) -> Result<()> {
        let typed: Vec<(&str, String)> = event_types
            .into_iter()
            .filter_map(|t| leading_segment(t).map(|code| (t, code)))
            .collect();
        let codes: Vec<String> = distinct(typed.iter().map(|(_, code)| code.as_str()));
        let application_ids = self.applications.find_ids_by_codes(&codes).await?;
        if application_ids.is_empty() {
            return Ok(());
        }
        let reach = SigningReach::for_caller(caller, &self.accounts).await?;
        if reach.is_super_admin() {
            return Ok(());
        }
        let anchor_scope = if caller.is_anchor() {
            Some(ApplicationScope::from_binding(
                self.principals
                    .find_application_binding(&caller.principal_id)
                    .await?,
            ))
        } else {
            None
        };
        for (event_type, code) in &typed {
            let Some(application_id) = application_ids.get(code) else {
                continue;
            };
            if anchor_scope
                .as_ref()
                .is_some_and(|scope| scope.allows(application_id))
            {
                continue;
            }
            if let Err(refusal) = reach.may_use_application(application_id, code) {
                return Err(PlatformError::forbidden_code(
                    "FORBIDDEN",
                    format!(
                        "an event of type '{event_type}' may be delivered signed by its application, \
                         which the caller may not sign as: {refusal}"
                    ),
                ));
            }
        }
        Ok(())
    }

    /// `Ok` when `caller` may cause every job's signed delivery; otherwise a
    /// 403 for the whole request, naming the first job refused.
    pub async fn check_jobs(&self, caller: &AuthContext, jobs: &[DispatchJob]) -> Result<()> {
        if jobs.is_empty() {
            return Ok(());
        }

        let subscription_ids: Vec<String> = distinct(
            jobs.iter()
                .filter_map(|j| non_blank(j.subscription_id.as_deref())),
        );
        let subscriptions: HashMap<String, Subscription> = self
            .subscriptions
            .find_by_ids(&subscription_ids)
            .await?
            .into_iter()
            .map(|s| (s.id.clone(), s))
            .collect();
        let subscription_of = |job: &DispatchJob| {
            non_blank(job.subscription_id.as_deref()).and_then(|id| subscriptions.get(id))
        };

        // 1. A non-anchor's job names only a subscription of its own client.
        if !caller.is_anchor() {
            for job in jobs {
                if let Some(sub) = subscription_of(job) {
                    if sub.client_id.is_none() || sub.client_id != job.client_id {
                        return Err(PlatformError::forbidden_code(
                            "FORBIDDEN",
                            format!("No access to subscription: {}", sub.id),
                        ));
                    }
                }
            }
        }

        // 2. The identity each job would be signed with.
        let connection_ids: Vec<String> =
            distinct(subscriptions.values().filter_map(connection_to_load));
        let connections: HashMap<String, Connection> = self
            .connections
            .find_by_ids(&connection_ids)
            .await?
            .into_iter()
            .map(|c| (c.id.clone(), c))
            .collect();
        let signers: Vec<Signer> = jobs
            .iter()
            .map(|job| {
                let sub = subscription_of(job);
                let connection = sub
                    .and_then(connection_to_load)
                    .and_then(|id| connections.get(id));
                signer_of(&job.code, sub, connection)
            })
            .collect();
        if signers.iter().all(|s| *s == Signer::Nobody) {
            return Ok(());
        }

        let reach = SigningReach::for_caller(caller, &self.accounts).await?;
        if reach.is_super_admin() {
            return Ok(());
        }
        let account_ids: Vec<String> = distinct(signers.iter().filter_map(|s| match s {
            Signer::Named { account_id, .. } => Some(account_id.as_str()),
            _ => None,
        }));
        let application_codes: Vec<String> = distinct(signers.iter().filter_map(|s| match s {
            Signer::OfApplication(code) => Some(code.as_str()),
            _ => None,
        }));
        let (accounts, application_ids) = tokio::try_join!(
            self.accounts.find_signing_accounts(&account_ids),
            self.applications.find_ids_by_codes(&application_codes),
        )?;

        for signer in &signers {
            let verdict = match signer {
                Signer::Named { account_id, .. } => match accounts.get(account_id) {
                    Some(account) => reach.may_use(account),
                    None => Ok(()),
                },
                Signer::OfApplication(code) => match application_ids.get(code) {
                    Some(id) => reach.may_use_application(id, code),
                    None => Ok(()),
                },
                Signer::Nobody => Ok(()),
            };
            if let Err(refusal) = verdict {
                return Err(PlatformError::forbidden_code(
                    "FORBIDDEN",
                    format!(
                        "dispatch job would be signed by an identity the caller may not use: {refusal}"
                    ),
                ));
            }
        }
        Ok(())
    }
}

fn non_blank(s: Option<&str>) -> Option<&str> {
    s.filter(|v| !v.trim().is_empty())
}

/// The distinct values, sorted (a stable query argument).
fn distinct<'a>(values: impl Iterator<Item = &'a str>) -> Vec<String> {
    values
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(str::to_string)
        .collect()
}
