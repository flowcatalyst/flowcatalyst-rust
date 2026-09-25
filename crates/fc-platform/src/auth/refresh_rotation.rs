//! Refresh-token rotation, in one place so the two refresh surfaces
//! (`/oauth/token` `refresh_token` and `/auth/refresh`) cannot drift (Go
//! `grantstore.Rotate`; Java `RefreshRotation`, 6a06a7f0 S2.5 and
//! 477db983):
//!
//! - **Single use, atomically.** The presented token is consumed by one
//!   `UPDATE … WHERE consumed_at IS NULL … RETURNING` in the same
//!   transaction as the replacement's insert and the `replacedBy` link, so
//!   of two concurrent presentations exactly one rotates.
//! - **Reuse detection** (OAuth 2.0 Security BCP §4.14.2): a presented
//!   token that is no longer valid but was rotated out means the family is
//!   presumed stolen — every token in it is revoked, logged at WARN.
//! - **Replay leeway** ([`REPLAY_LEEWAY`]): a token rotated out moments ago
//!   is far more often its own client racing itself (the Laravel and
//!   TypeScript SDKs refresh per request with no lock) or retrying a lost
//!   response than a thief. Within the leeway, and only while the family is
//!   intact (the replacement still valid), it rotates again into a sibling
//!   in the same family. Later, or once the family is revoked, it is reuse.
//! - **Binding.** A token issued to an OAuth client is refreshed only by
//!   that client; a refusal consumes nothing.
//! - **Lineage.** The replacement keeps the binding, scopes and accessible
//!   clients, stays in the family, and inherits the presented token's
//!   expiry — the family's absolute cap — never a fresh one.
//!
//! Token rows are auth infrastructure (CLAUDE.md: "Auth/OIDC token
//! storage"): written straight to the repository, no use case or event.

use async_trait::async_trait;
use chrono::{Duration, Utc};
use tracing::warn;

use crate::shared::error::Result;
use crate::{RefreshToken, RefreshTokenRepository};

/// How long after a token is rotated out a second presentation of it is
/// still its own client racing or retrying, not a replay.
pub const REPLAY_LEEWAY: Duration = Duration::seconds(10);

/// A completed rotation.
#[derive(Debug)]
pub struct Rotated {
    /// The token that was presented (now consumed).
    pub stored: RefreshToken,
    /// The replacement's raw value: handed to the caller once, never stored.
    pub new_raw: String,
    /// The replacement as stored.
    pub replacement: RefreshToken,
}

/// Why nothing was rotated.
#[derive(Debug, PartialEq, Eq)]
pub enum Rejection {
    /// Never issued, expired or revoked, and not a replay of a rotated-out
    /// token.
    Unknown,
    /// A rotated-out token was presented again: its family was revoked.
    ReuseDetected { family: String, revoked: u64 },
    /// The token is bound to another OAuth client than the one presenting
    /// it (or to one, when none is presenting it).
    Refused { token_client_id: String },
}

/// The storage rotation needs; [`RefreshTokenRepository`] in production.
#[async_trait]
pub trait RefreshTokenStore: Send + Sync {
    async fn find_valid_by_hash(&self, token_hash: &str) -> Result<Option<RefreshToken>>;
    async fn find_by_hash(&self, token_hash: &str) -> Result<Option<RefreshToken>>;
    async fn consume_and_replace(
        &self,
        token_hash: &str,
        family: &str,
        replacement: &RefreshToken,
    ) -> Result<bool>;
    async fn insert(&self, token: &RefreshToken) -> Result<()>;
    async fn revoke_all_in_family(&self, family: &str) -> Result<u64>;
}

#[async_trait]
impl RefreshTokenStore for RefreshTokenRepository {
    async fn find_valid_by_hash(&self, token_hash: &str) -> Result<Option<RefreshToken>> {
        RefreshTokenRepository::find_valid_by_hash(self, token_hash).await
    }
    async fn find_by_hash(&self, token_hash: &str) -> Result<Option<RefreshToken>> {
        RefreshTokenRepository::find_by_hash(self, token_hash).await
    }
    async fn consume_and_replace(
        &self,
        token_hash: &str,
        family: &str,
        replacement: &RefreshToken,
    ) -> Result<bool> {
        RefreshTokenRepository::consume_and_replace(self, token_hash, family, replacement).await
    }
    async fn insert(&self, token: &RefreshToken) -> Result<()> {
        RefreshTokenRepository::insert(self, token).await
    }
    async fn revoke_all_in_family(&self, family: &str) -> Result<u64> {
        RefreshTokenRepository::revoke_all_in_family(self, family).await
    }
}

/// Consume `raw` and issue its replacement. `requesting_client_id` is the
/// authenticated OAuth client presenting it, `None` when the caller is no
/// client (`/auth/refresh`) — then only a token issued outside any client
/// rotates.
pub async fn rotate(
    store: &dyn RefreshTokenStore,
    raw: &str,
    requesting_client_id: Option<&str>,
) -> Result<std::result::Result<Rotated, Rejection>> {
    let hash = RefreshToken::hash_token(raw);
    let Some(stored) = store.find_valid_by_hash(&hash).await? else {
        return not_valid(store, &hash, requesting_client_id).await;
    };
    if let Some(refused) = refused(&stored, requesting_client_id) {
        return Ok(Err(refused));
    }
    let (new_raw, replacement) = stored.successor();
    if !store
        .consume_and_replace(&hash, &stored.family(), &replacement)
        .await?
    {
        // Someone else consumed it between our read and our UPDATE (which
        // waited on their row lock): it now reads as rotated out moments
        // ago — the leeway case, handled like any second presentation.
        return not_valid(store, &hash, requesting_client_id).await;
    }
    Ok(Ok(Rotated {
        stored,
        new_raw,
        replacement,
    }))
}

fn refused(token: &RefreshToken, requesting_client_id: Option<&str>) -> Option<Rejection> {
    match token.oauth_client_id.as_deref() {
        Some(bound) if Some(bound) != requesting_client_id => Some(Rejection::Refused {
            token_client_id: bound.to_string(),
        }),
        _ => None,
    }
}

/// A presented token that is not valid. Rotated out within the leeway with
/// its family intact: a sibling replacement. Rotated out earlier, or its
/// family already revoked: reuse — the family is revoked. Anything else is
/// simply unknown.
async fn not_valid(
    store: &dyn RefreshTokenStore,
    hash: &str,
    requesting_client_id: Option<&str>,
) -> Result<std::result::Result<Rotated, Rejection>> {
    let Some(prior) = store.find_by_hash(hash).await?.filter(|t| t.was_replaced()) else {
        return Ok(Err(Rejection::Unknown));
    };
    let within_leeway = prior
        .revoked_at
        .is_some_and(|at| at >= Utc::now() - REPLAY_LEEWAY);
    let family_intact = match prior.replaced_by.as_deref() {
        Some(next) => store.find_valid_by_hash(next).await?.is_some(),
        None => false,
    };
    if within_leeway && family_intact {
        if let Some(refused) = refused(&prior, requesting_client_id) {
            return Ok(Err(refused));
        }
        let (new_raw, sibling) = prior.successor();
        store.insert(&sibling).await?;
        return Ok(Ok(Rotated {
            stored: prior,
            new_raw,
            replacement: sibling,
        }));
    }
    let family = prior.family();
    let revoked = store.revoke_all_in_family(&family).await?;
    warn!(
        family = %family,
        token = %prior.id,
        principal = %prior.principal_id,
        revoked,
        "refresh token reuse detected; family revoked"
    );
    Ok(Err(Rejection::ReuseDetected { family, revoked }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// An in-memory store with the repository's semantics.
    #[derive(Default)]
    struct Memory {
        tokens: Mutex<Vec<RefreshToken>>,
        /// Consumed before `consume_and_replace` runs: the race loser.
        lose_next_race: Mutex<bool>,
    }

    impl Memory {
        fn with(token: RefreshToken) -> Self {
            let m = Self::default();
            m.tokens.lock().unwrap().push(token);
            m
        }
        fn get(&self, hash: &str) -> RefreshToken {
            self.tokens
                .lock()
                .unwrap()
                .iter()
                .find(|t| t.token_hash == hash)
                .cloned()
                .unwrap()
        }
        fn live(&self) -> usize {
            self.tokens
                .lock()
                .unwrap()
                .iter()
                .filter(|t| t.is_valid())
                .count()
        }
        fn consume(tokens: &mut [RefreshToken], hash: &str, family: &str, next: &str) -> bool {
            match tokens
                .iter_mut()
                .find(|t| t.token_hash == hash && t.is_valid())
            {
                Some(t) => {
                    t.revoke();
                    t.replaced_by = Some(next.to_string());
                    t.token_family.get_or_insert_with(|| family.to_string());
                    true
                }
                None => false,
            }
        }
        /// Rotation happened `ago` in the past.
        fn backdate(&self, hash: &str, ago: Duration) {
            let mut tokens = self.tokens.lock().unwrap();
            let t = tokens.iter_mut().find(|t| t.token_hash == hash).unwrap();
            t.revoked_at = Some(Utc::now() - ago);
        }
    }

    #[async_trait]
    impl RefreshTokenStore for Memory {
        async fn find_valid_by_hash(&self, hash: &str) -> Result<Option<RefreshToken>> {
            Ok(self
                .tokens
                .lock()
                .unwrap()
                .iter()
                .find(|t| t.token_hash == hash && t.is_valid())
                .cloned())
        }
        async fn find_by_hash(&self, hash: &str) -> Result<Option<RefreshToken>> {
            Ok(self
                .tokens
                .lock()
                .unwrap()
                .iter()
                .find(|t| t.token_hash == hash)
                .cloned())
        }
        async fn consume_and_replace(
            &self,
            hash: &str,
            family: &str,
            replacement: &RefreshToken,
        ) -> Result<bool> {
            let mut tokens = self.tokens.lock().unwrap();
            if std::mem::take(&mut *self.lose_next_race.lock().unwrap()) {
                // A concurrent winner rotated it first.
                let (_, winner) = tokens
                    .iter()
                    .find(|t| t.token_hash == hash)
                    .unwrap()
                    .successor();
                Self::consume(&mut tokens, hash, family, &winner.token_hash);
                tokens.push(winner);
                return Ok(false);
            }
            if !Self::consume(&mut tokens, hash, family, &replacement.token_hash) {
                return Ok(false);
            }
            tokens.push(replacement.clone());
            Ok(true)
        }
        async fn insert(&self, token: &RefreshToken) -> Result<()> {
            self.tokens.lock().unwrap().push(token.clone());
            Ok(())
        }
        async fn revoke_all_in_family(&self, family: &str) -> Result<u64> {
            let mut n = 0;
            for t in self.tokens.lock().unwrap().iter_mut() {
                if t.token_family.as_deref() == Some(family) && !t.revoked {
                    t.revoke();
                    n += 1;
                }
            }
            Ok(n)
        }
    }

    fn issued(client: Option<&str>) -> (String, RefreshToken) {
        let (raw, token) = RefreshToken::generate_token_pair("prn_1");
        let token = match client {
            Some(c) => token.with_oauth_client(c),
            None => token,
        };
        (raw, token.with_expiry(Duration::days(3)))
    }

    #[tokio::test]
    async fn rotation_consumes_and_inherits_the_expiry() {
        let (raw, token) = issued(Some("oc_planner"));
        let store = Memory::with(token.clone());
        let rotated = rotate(&store, &raw, Some("oc_planner"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rotated.stored.id, token.id);
        assert_eq!(rotated.replacement.expires_at, token.expires_at);
        assert_eq!(rotated.replacement.token_family, token.token_family);
        assert_eq!(
            RefreshToken::hash_token(&rotated.new_raw),
            rotated.replacement.token_hash
        );
        let old = store.get(&token.token_hash);
        assert!(old.revoked);
        assert_eq!(old.replaced_by, Some(rotated.replacement.token_hash));
        assert_eq!(store.live(), 1);
    }

    #[tokio::test]
    async fn a_token_bound_elsewhere_is_refused_and_not_consumed() {
        let (raw, token) = issued(Some("oc_planner"));
        let store = Memory::with(token.clone());
        for requesting in [None, Some("oc_other")] {
            assert_eq!(
                rotate(&store, &raw, requesting).await.unwrap().unwrap_err(),
                Rejection::Refused {
                    token_client_id: "oc_planner".to_string()
                }
            );
        }
        assert!(store.get(&token.token_hash).is_valid());
    }

    #[tokio::test]
    async fn an_unknown_token_is_unknown() {
        let store = Memory::default();
        assert_eq!(
            rotate(&store, "nope", None).await.unwrap().unwrap_err(),
            Rejection::Unknown
        );
    }

    /// Two presentations race: both succeed, exactly one consumes, and the
    /// family stays whole (Java 477db983).
    #[tokio::test]
    async fn a_client_racing_itself_is_not_signed_out() {
        let (raw, token) = issued(None);
        let store = Memory::with(token.clone());
        *store.lose_next_race.lock().unwrap() = true;
        let sibling = rotate(&store, &raw, None).await.unwrap().unwrap();
        assert_eq!(sibling.replacement.token_family, token.token_family);
        assert_eq!(sibling.replacement.expires_at, token.expires_at);
        // The winner's token and the sibling are both live.
        assert_eq!(store.live(), 2);
    }

    /// A sequential retry within the leeway also gets a sibling.
    #[tokio::test]
    async fn a_retry_within_the_leeway_gets_a_sibling() {
        let (raw, token) = issued(None);
        let store = Memory::with(token);
        let first = rotate(&store, &raw, None).await.unwrap().unwrap();
        let second = rotate(&store, &raw, None).await.unwrap().unwrap();
        assert_ne!(first.replacement.id, second.replacement.id);
        assert_eq!(store.live(), 2);
    }

    /// A replay after the leeway is reuse: the whole family — the
    /// legitimate replacement too — is revoked.
    #[tokio::test]
    async fn a_replay_after_the_leeway_revokes_the_family() {
        let (raw, token) = issued(None);
        let store = Memory::with(token.clone());
        let rotated = rotate(&store, &raw, None).await.unwrap().unwrap();
        store.backdate(&token.token_hash, REPLAY_LEEWAY + Duration::seconds(1));

        let err = rotate(&store, &raw, None).await.unwrap().unwrap_err();
        assert_eq!(
            err,
            Rejection::ReuseDetected {
                family: token.id.clone(),
                revoked: 1
            }
        );
        assert!(!store.get(&rotated.replacement.token_hash).is_valid());
        assert_eq!(store.live(), 0);
        // The replacement is dead too.
        assert!(matches!(
            rotate(&store, &rotated.new_raw, None)
                .await
                .unwrap()
                .unwrap_err(),
            Rejection::ReuseDetected { .. } | Rejection::Unknown
        ));
    }

    /// Within the leeway but with the family already revoked, a replay
    /// never revives it.
    #[tokio::test]
    async fn a_revoked_family_is_never_revived() {
        let (raw, token) = issued(None);
        let store = Memory::with(token.clone());
        rotate(&store, &raw, None).await.unwrap().unwrap();
        store.revoke_all_in_family(&token.id).await.unwrap();
        assert!(matches!(
            rotate(&store, &raw, None).await.unwrap().unwrap_err(),
            Rejection::ReuseDetected { .. }
        ));
        assert_eq!(store.live(), 0);
    }

    /// A legacy token with no family roots one at its own id, so a replay
    /// of it after rotation is still caught.
    #[tokio::test]
    async fn a_legacy_token_roots_a_family() {
        let (raw, mut token) = issued(None);
        token.token_family = None;
        let store = Memory::with(token.clone());
        let rotated = rotate(&store, &raw, None).await.unwrap().unwrap();
        assert_eq!(
            rotated.replacement.token_family.as_deref(),
            Some(token.id.as_str())
        );
        assert_eq!(
            store.get(&token.token_hash).token_family.as_deref(),
            Some(token.id.as_str())
        );
        store.backdate(&token.token_hash, Duration::minutes(5));
        assert!(matches!(
            rotate(&store, &raw, None).await.unwrap().unwrap_err(),
            Rejection::ReuseDetected { .. }
        ));
    }
}
