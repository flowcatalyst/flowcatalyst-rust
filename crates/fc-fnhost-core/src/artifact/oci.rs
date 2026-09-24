//! `oci://<registry>/<repository>` (Java `OciArtifactStore`): the blob is
//! addressed by the version's digest alone; a tag or `@digest` suffix is
//! rejected. Auth, in order: anonymous; on a 401 with a `Bearer` challenge,
//! the token dance, retried once; on a 401 with a `Basic` challenge, Basic,
//! retried once. `Authorization` is never forwarded across a redirect to
//! another origin (registries redirect blob reads to object storage), so
//! redirects are followed by hand, at most one.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine;
use reqwest::header::{HeaderMap, AUTHORIZATION, CONTENT_LENGTH, LOCATION, WWW_AUTHENTICATE};
use reqwest::{Client, StatusCode, Url};

use super::cache::HttpBody;
use super::{ArtifactError, Source, SourceStream};
use crate::digest::Digest;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Java's `HttpRequest.timeout`: the wait for response headers.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// HTTP Basic credentials per registry authority (`ghcr.io`,
/// `localhost:5000`). The host wires none, exactly as Java's `FnHost` does
/// (`RegistryCredentials.none()`): only anonymous pulls and anonymous
/// bearer tokens work until an owner decision says otherwise.
#[derive(Clone, Default)]
pub struct RegistryCredentials {
    by_host: HashMap<String, (String, String)>,
}

impl RegistryCredentials {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn fixed(by_host: HashMap<String, (String, String)>) -> Self {
        Self { by_host }
    }

    fn basic_header(&self, registry: &str) -> Option<String> {
        self.by_host.get(registry).map(|(user, password)| {
            let raw = format!("{user}:{password}");
            format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(raw)
            )
        })
    }
}

impl std::fmt::Debug for RegistryCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "RegistryCredentials[{} registr(ies), passwords=***]",
            self.by_host.len()
        )
    }
}

#[derive(Clone)]
pub struct OciSource {
    client: Client,
    credentials: Arc<RegistryCredentials>,
}

impl OciSource {
    pub fn new(credentials: RegistryCredentials) -> Self {
        let client = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("a plain reqwest client builds");
        Self {
            client,
            credentials: Arc::new(credentials),
        }
    }

    async fn get(
        &self,
        url: Url,
        authorization: Option<&str>,
    ) -> Result<reqwest::Response, ArtifactError> {
        let mut request = self.client.get(url);
        if let Some(authorization) = authorization {
            request = request.header(AUTHORIZATION, authorization);
        }
        match tokio::time::timeout(REQUEST_TIMEOUT, request.send()).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(e)) => Err(ArtifactError::transport(e)),
            Err(_) => Err(ArtifactError::Transport(
                "registry request timed out".into(),
            )),
        }
    }

    /// `GET url`, following at most one redirect by hand, forwarding
    /// `authorization` only to the same origin (host and port).
    async fn get_following_redirect(
        &self,
        url: Url,
        authorization: Option<&str>,
    ) -> Result<reqwest::Response, ArtifactError> {
        let response = self.get(url.clone(), authorization).await?;
        if !response.status().is_redirection() {
            return Ok(response);
        }
        let location = response
            .headers()
            .get(LOCATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| ArtifactError::Transport("redirect with no Location header".into()))?;
        let target = url.join(location).map_err(ArtifactError::transport)?;
        let forwarded = if same_origin(&url, &target) {
            authorization
        } else {
            None
        };
        self.get(target, forwarded).await
    }

    async fn authenticate(
        &self,
        reference: &OciRef,
        challenge: &str,
    ) -> Result<Option<String>, ArtifactError> {
        let lower = challenge.to_ascii_lowercase();
        if lower.starts_with("bearer") {
            return self.bearer_token(reference, challenge).await;
        }
        if lower.starts_with("basic") {
            return Ok(self.credentials.basic_header(&reference.registry));
        }
        Ok(None)
    }

    async fn bearer_token(
        &self,
        reference: &OciRef,
        challenge: &str,
    ) -> Result<Option<String>, ArtifactError> {
        let params = challenge_params(challenge);
        let Some(realm) = params.get("realm") else {
            return Ok(None);
        };
        let service = params.get("service").map(String::as_str).unwrap_or("");
        let scope = format!("repository:{}:pull", reference.repository);
        let query = format!(
            "service={}&scope={}",
            url_encode(service),
            url_encode(&scope)
        );
        let separator = if realm.contains('?') { '&' } else { '?' };
        let token_url =
            Url::parse(&format!("{realm}{separator}{query}")).map_err(ArtifactError::transport)?;
        let mut request = self.client.get(token_url);
        if let Some(basic) = self.credentials.basic_header(&reference.registry) {
            request = request.header(AUTHORIZATION, basic);
        }
        let response = match tokio::time::timeout(REQUEST_TIMEOUT, request.send()).await {
            Ok(Ok(response)) => response,
            Ok(Err(e)) => return Err(ArtifactError::transport(e)),
            Err(_) => return Err(ArtifactError::Transport("token request timed out".into())),
        };
        if response.status() != StatusCode::OK {
            return Ok(None);
        }
        let body: serde_json::Value = response.json().await.map_err(ArtifactError::transport)?;
        let token = match (body.get("token"), body.get("access_token")) {
            (Some(serde_json::Value::String(t)), _) => Some(t.clone()),
            (_, Some(serde_json::Value::String(t))) => Some(t.clone()),
            _ => None,
        };
        Ok(token.map(|t| format!("Bearer {t}")))
    }
}

#[async_trait]
impl Source for OciSource {
    async fn open(
        &self,
        artifact_ref: &str,
        expected: &Digest,
        _: Option<&str>,
    ) -> Result<SourceStream, ArtifactError> {
        let reference = OciRef::parse(artifact_ref)?;
        let blob = reference.blob_url(expected)?;
        let response = self.get_following_redirect(blob.clone(), None).await?;
        if response.status() == StatusCode::OK {
            return Ok(to_source_stream(response));
        }
        if response.status() != StatusCode::UNAUTHORIZED {
            return Err(status_error(response.status()));
        }
        let challenge = response
            .headers()
            .get(WWW_AUTHENTICATE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();
        drop(response);
        let Some(authorization) = self.authenticate(&reference, &challenge).await? else {
            return Err(ArtifactError::Unauthorized);
        };
        let retry = self
            .get_following_redirect(blob, Some(&authorization))
            .await?;
        if retry.status() == StatusCode::OK {
            return Ok(to_source_stream(retry));
        }
        Err(status_error(retry.status()))
    }
}

fn to_source_stream(response: reqwest::Response) -> SourceStream {
    let content_length = content_length(response.headers());
    SourceStream {
        body: Box::new(HttpBody(response)),
        content_length,
    }
}

pub(crate) fn content_length(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn status_error(status: StatusCode) -> ArtifactError {
    match status.as_u16() {
        401 | 403 => ArtifactError::Unauthorized,
        404 => ArtifactError::NotFound,
        other => ArtifactError::Transport(format!("registry returned HTTP {other}")),
    }
}

fn same_origin(a: &Url, b: &Url) -> bool {
    a.host_str() == b.host_str() && a.port_or_known_default() == b.port_or_known_default()
}

/// `(\w+)="([^"]*)"` over the challenge.
fn challenge_params(challenge: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    let bytes = challenge.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            if challenge[i..].starts_with("=\"") {
                let value_start = i + 2;
                if let Some(end) = challenge[value_start..].find('"') {
                    params.insert(
                        challenge[start..i].to_owned(),
                        challenge[value_start..value_start + end].to_owned(),
                    );
                    i = value_start + end + 1;
                    continue;
                }
            }
        } else {
            i += 1;
        }
    }
    params
}

/// `URLEncoder.encode(value, UTF_8)`.
fn url_encode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

#[derive(Debug, PartialEq, Eq)]
struct OciRef {
    host: String,
    registry: String,
    repository: String,
}

impl OciRef {
    /// `oci://<registry>/<repository>`: no tag, no `@digest`, no fragment.
    fn parse(artifact_ref: &str) -> Result<Self, ArtifactError> {
        let rest = artifact_ref
            .strip_prefix("oci:")
            .ok_or_else(|| ArtifactError::BadRef("not an oci:// reference".into()))?;
        let (rest, fragment) = match rest.split_once('#') {
            Some((rest, fragment)) => (rest, Some(fragment)),
            None => (rest, None),
        };
        let rest = rest.split('?').next().unwrap_or("");
        let after = rest.strip_prefix("//").ok_or_else(|| {
            ArtifactError::BadRef("oci:// reference must name a registry host".into())
        })?;
        let (authority, path) = after.split_at(after.find('/').unwrap_or(after.len()));
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        };
        let port_ok = port.is_none_or(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
        if host.is_empty() || !port_ok {
            return Err(ArtifactError::BadRef(
                "oci:// reference must name a registry host".into(),
            ));
        }
        let registry = match port {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        };
        if path.len() < 2 || !path.starts_with('/') {
            return Err(ArtifactError::BadRef(
                "oci:// reference must name a repository".into(),
            ));
        }
        let repository = super::percent_decode(&path[1..])?;
        if repository.contains('@') || repository.contains(':') || fragment.is_some() {
            return Err(ArtifactError::BadRef(
                "oci:// reference must not carry a tag or @digest — the blob is addressed by Digest alone".into(),
            ));
        }
        Ok(Self {
            host: host.to_owned(),
            registry,
            repository,
        })
    }

    /// `http://` only for the local registries dev and tests point at.
    fn blob_url(&self, digest: &Digest) -> Result<Url, ArtifactError> {
        let use_http = self.host.eq_ignore_ascii_case("localhost") || self.host == "127.0.0.1";
        let scheme = if use_http { "http" } else { "https" };
        Url::parse(&format!(
            "{scheme}://{}/v2/{}/blobs/{}",
            self.registry,
            self.repository,
            digest.value()
        ))
        .map_err(|e| ArtifactError::BadRef(format!("malformed URI: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact::{ArtifactCache, DEFAULT_MAX_BYTES};
    use axum::extract::{Query, State};
    use axum::http::{HeaderMap as AxumHeaders, StatusCode as AxumStatus};
    use axum::response::IntoResponse;
    use axum::routing::get;
    use axum::Router;
    use sha2::{Digest as _, Sha256};

    #[test]
    fn references_parse_like_java() {
        let r = OciRef::parse("oci://localhost:5000/team/fn").unwrap();
        assert_eq!(r.host, "localhost");
        assert_eq!(r.registry, "localhost:5000");
        assert_eq!(r.repository, "team/fn");
        assert!(OciRef::parse("oci://ghcr.io/team/fn:v1").is_err());
        assert!(OciRef::parse("oci://ghcr.io/team/fn@sha256:ab").is_err());
        assert!(OciRef::parse("oci://ghcr.io/team/fn#x").is_err());
        assert!(OciRef::parse("oci://ghcr.io").is_err());
        assert!(OciRef::parse("oci:///repo").is_err());
        let d = Digest::from_sha256(&[1; 32]);
        assert!(r
            .blob_url(&d)
            .unwrap()
            .as_str()
            .starts_with("http://localhost:5000/v2/team/fn/blobs/sha256:"));
        assert!(OciRef::parse("oci://ghcr.io/a/b")
            .unwrap()
            .blob_url(&d)
            .unwrap()
            .as_str()
            .starts_with("https://"));
    }

    #[test]
    fn challenge_params_follow_the_java_regex() {
        let p = challenge_params(
            r#"Bearer realm="http://auth/token",service="registry.local",scope="x""#,
        );
        assert_eq!(p["realm"], "http://auth/token");
        assert_eq!(p["service"], "registry.local");
    }

    #[derive(Clone)]
    struct Registry {
        blob: Vec<u8>,
        realm: String,
    }

    async fn blob(State(r): State<Registry>, headers: AxumHeaders) -> axum::response::Response {
        match headers.get("authorization").and_then(|v| v.to_str().ok()) {
            Some("Bearer t0k3n") => (AxumStatus::OK, r.blob.clone()).into_response(),
            _ => (
                AxumStatus::UNAUTHORIZED,
                [(
                    "WWW-Authenticate",
                    format!(r#"Bearer realm="{}",service="reg""#, r.realm),
                )],
            )
                .into_response(),
        }
    }

    async fn token(Query(q): Query<HashMap<String, String>>) -> axum::response::Response {
        if q.get("scope").map(String::as_str) == Some("repository:team/fn:pull")
            && q.get("service").map(String::as_str) == Some("reg")
        {
            axum::Json(serde_json::json!({"token": "t0k3n"})).into_response()
        } else {
            AxumStatus::FORBIDDEN.into_response()
        }
    }

    #[tokio::test]
    async fn anonymous_bearer_token_dance_fetches_the_blob() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let content = b"wasm blob".to_vec();
        let digest = Digest::from_sha256(&Sha256::digest(&content).into());
        let state = Registry {
            blob: content.clone(),
            realm: format!("http://127.0.0.1:{port}/token"),
        };
        let app = Router::new()
            .route(&format!("/v2/team/fn/blobs/{}", digest.value()), get(blob))
            .route("/token", get(token))
            .with_state(state);
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let dir = tempfile::tempdir().unwrap();
        let cache = ArtifactCache::new(dir.path(), DEFAULT_MAX_BYTES).unwrap();
        let source = OciSource::new(RegistryCredentials::none());
        let fetched = cache
            .fetch(
                &source,
                &format!("oci://127.0.0.1:{port}/team/fn"),
                &digest,
                None,
            )
            .await
            .unwrap();
        assert_eq!(std::fs::read(fetched.file).unwrap(), content);

        let missing = Digest::from_sha256(&[7; 32]);
        let err = cache
            .fetch(
                &source,
                &format!("oci://127.0.0.1:{port}/team/fn"),
                &missing,
                None,
            )
            .await
            .unwrap_err();
        assert_eq!(err, ArtifactError::NotFound);
    }
}
