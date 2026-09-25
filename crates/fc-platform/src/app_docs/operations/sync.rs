//! An application replaces its documentation pages (Go
//! `sdksync/docs_sync.go` + `appdocs.ReplaceForApplication`): at most 100
//! kebab-case slugs, 512 KiB a page, 4 MiB in all; every listed page is
//! written in order, every other page of the application removed.
//!
//! Go writes the pages without an event or audit row. Rust routes the write
//! through a unit of work, so it carries a `platform:admin:app-docs:synced`
//! event (a Rust addition); the audit row names the slugs, not the content.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

use crate::app_docs::entity::{doc_title, AppDoc, AppDocsReplacement};
use crate::app_docs::repository::AppDocsRepository;
use crate::impl_domain_event;
use crate::usecase::domain_event::EventMetadata;
use crate::usecase::{ExecutionContext, UnitOfWork, UseCase, UseCaseError, UseCaseResult};

pub const MAX_DOCS: usize = 100;
pub const MAX_DOC_BYTES: usize = 512 * 1024;
pub const MAX_TOTAL_BYTES: usize = 4 << 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppDocsSynced {
    #[serde(flatten)]
    pub metadata: EventMetadata,
    pub application_code: String,
    pub created: u32,
    pub updated: u32,
    pub deleted: u32,
    pub synced_codes: Vec<String>,
}

impl_domain_event!(AppDocsSynced);

impl AppDocsSynced {
    pub const EVENT_TYPE: &'static str = "platform:admin:app-docs:synced";
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncDocInput {
    pub slug: String,
    pub title: Option<String>,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncAppDocsCommand {
    pub application_id: String,
    pub application_code: String,
    /// The pages, in order. Not in the audit row (up to 4 MiB); `slugs` is.
    #[serde(skip)]
    pub docs: Vec<SyncDocInput>,
    pub slugs: Vec<String>,
}

impl crate::usecase::AuditMasked for SyncAppDocsCommand {}

/// Go `slugPattern`: `^[a-z0-9][a-z0-9-]*$`.
fn is_slug(s: &str) -> bool {
    let mut b = s.bytes();
    matches!(b.next(), Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit())
        && b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

pub struct SyncAppDocsUseCase<U: UnitOfWork> {
    repo: Arc<AppDocsRepository>,
    unit_of_work: Arc<U>,
}

impl<U: UnitOfWork> SyncAppDocsUseCase<U> {
    pub fn new(repo: Arc<AppDocsRepository>, unit_of_work: Arc<U>) -> Self {
        Self { repo, unit_of_work }
    }
}

#[async_trait]
impl<U: UnitOfWork> UseCase for SyncAppDocsUseCase<U> {
    type Command = SyncAppDocsCommand;
    type Event = AppDocsSynced;

    async fn validate(&self, c: &SyncAppDocsCommand) -> Result<(), UseCaseError> {
        if c.docs.len() > MAX_DOCS {
            return Err(UseCaseError::validation(
                "TOO_MANY_DOCS",
                "an application may sync at most 100 documentation pages",
            ));
        }
        let mut seen = HashSet::new();
        let mut total = 0usize;
        for d in &c.docs {
            let slug = d.slug.trim();
            if !is_slug(slug) {
                return Err(UseCaseError::validation(
                    "SLUG_INVALID",
                    format!(
                        "doc slug {slug} must be kebab-case (lowercase letters, digits, hyphens)"
                    ),
                ));
            }
            if !seen.insert(slug) {
                return Err(UseCaseError::validation(
                    "SLUG_DUPLICATE",
                    format!("doc slug {slug} appears more than once"),
                ));
            }
            if d.content.len() > MAX_DOC_BYTES {
                return Err(UseCaseError::validation(
                    "DOC_TOO_LARGE",
                    format!("doc {slug} exceeds 512KB"),
                ));
            }
            total += d.content.len();
            if total > MAX_TOTAL_BYTES {
                return Err(UseCaseError::validation(
                    "PAYLOAD_TOO_LARGE",
                    "documentation sync exceeds 4MB total",
                ));
            }
        }
        Ok(())
    }

    async fn authorize(
        &self,
        _c: &SyncAppDocsCommand,
        _ctx: &ExecutionContext,
    ) -> Result<(), UseCaseError> {
        Ok(())
    }

    async fn execute(
        &self,
        command: SyncAppDocsCommand,
        ctx: ExecutionContext,
    ) -> UseCaseResult<AppDocsSynced> {
        let existing: HashSet<String> = match self
            .repo
            .slugs_for_application(&command.application_id)
            .await
        {
            Ok(s) => s.into_iter().collect(),
            Err(e) => return UseCaseResult::failure(e.into()),
        };
        let now = chrono::Utc::now();
        let (mut created, mut updated) = (0u32, 0u32);
        let mut docs = Vec::with_capacity(command.docs.len());
        let mut synced = Vec::with_capacity(command.docs.len());
        for (i, d) in command.docs.iter().enumerate() {
            let slug = d.slug.trim().to_string();
            if existing.contains(&slug) {
                updated += 1;
            } else {
                created += 1;
            }
            docs.push(AppDoc {
                id: crate::shared::tsid::generate_with_prefix("doc"),
                application_id: command.application_id.clone(),
                title: doc_title(&slug, d.title.as_deref(), &d.content),
                content: d.content.clone(),
                position: i as i32,
                slug: slug.clone(),
                created_at: now,
                updated_at: now,
            });
            synced.push(slug);
        }
        let listed: HashSet<&str> = synced.iter().map(String::as_str).collect();
        let removed: Vec<String> = existing
            .iter()
            .filter(|s| !listed.contains(s.as_str()))
            .cloned()
            .collect();
        let event = AppDocsSynced {
            metadata: EventMetadata::from_ctx(
                &ctx,
                AppDocsSynced::EVENT_TYPE,
                "1.0",
                "platform:admin",
                format!("platform.application.{}", command.application_code),
                format!("platform:application:{}", command.application_code),
            ),
            application_code: command.application_code.clone(),
            created,
            updated,
            deleted: removed.len() as u32,
            synced_codes: synced,
        };
        let replacement = AppDocsReplacement {
            application_id: command.application_id.clone(),
            docs,
            removed_slugs: removed,
        };
        self.unit_of_work
            .commit(&replacement, &*self.repo, event, &command)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::is_slug;

    #[test]
    fn slugs_are_kebab_case() {
        assert!(is_slug("getting-started"));
        assert!(is_slug("2fa"));
        assert!(!is_slug("-x"));
        assert!(!is_slug("Getting"));
        assert!(!is_slug(""));
    }
}
