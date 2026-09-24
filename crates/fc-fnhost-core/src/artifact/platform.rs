//! `platform://<functionId>/<hex>` (Java host `PlatformArtifactStore`):
//! `GET /control/functions/artifacts/{versionId}` with the host's bearer,
//! re-minted once on a 401, streamed through the shared cache. The route is
//! keyed by version id, so a fetch without one is `VersionRequired`.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::AUTHORIZATION;
use reqwest::{Client, StatusCode};

use super::cache::HttpBody;
use super::oci::content_length;
use super::{ArtifactError, Source, SourceStream};
use crate::digest::Digest;
use crate::token::TokenSource;

/// Java's `HttpRequest.timeout`: the wait for response headers. The body is
/// bounded by the cache's idle timeout instead.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub struct PlatformSource {
    client: Client,
    platform_url: String,
    token_source: Arc<TokenSource>,
}

impl PlatformSource {
    /// `client` must not carry a total request timeout (a 256 MiB body can
    /// take longer than 30 s); only the header wait is bounded here.
    pub fn new(
        client: Client,
        platform_url: impl Into<String>,
        token_source: Arc<TokenSource>,
    ) -> Self {
        Self {
            client,
            platform_url: platform_url.into(),
            token_source,
        }
    }

    async fn get(&self, version_id: &str, token: &str) -> Result<reqwest::Response, ArtifactError> {
        let url = format!(
            "{}/control/functions/artifacts/{version_id}",
            self.platform_url
        );
        let request = self
            .client
            .get(url)
            .header(AUTHORIZATION, format!("Bearer {token}"));
        match tokio::time::timeout(REQUEST_TIMEOUT, request.send()).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(e)) => Err(ArtifactError::transport(e)),
            Err(_) => Err(ArtifactError::Transport(
                "control plane artifact request timed out".into(),
            )),
        }
    }
}

#[async_trait]
impl Source for PlatformSource {
    async fn open(
        &self,
        _: &str,
        _: &Digest,
        version_id: Option<&str>,
    ) -> Result<SourceStream, ArtifactError> {
        let version_id = match version_id {
            Some(id) if !crate::java::is_blank(id) => id,
            _ => return Err(ArtifactError::VersionRequired),
        };
        let token = self
            .token_source
            .token()
            .await
            .map_err(ArtifactError::transport)?;
        let mut response = self.get(version_id, &token).await?;
        if response.status() == StatusCode::UNAUTHORIZED {
            let refreshed = self
                .token_source
                .refresh()
                .await
                .map_err(ArtifactError::transport)?;
            response = self.get(version_id, &refreshed).await?;
        }
        match response.status() {
            StatusCode::OK => Ok(SourceStream {
                content_length: content_length(response.headers()),
                body: Box::new(HttpBody(response)),
            }),
            StatusCode::UNAUTHORIZED => Err(ArtifactError::Unauthorized),
            StatusCode::NOT_FOUND => Err(ArtifactError::NotFound),
            other => Err(ArtifactError::Transport(format!(
                "control plane returned HTTP {} for GET /control/functions/artifacts/{version_id}",
                other.as_u16()
            ))),
        }
    }
}
