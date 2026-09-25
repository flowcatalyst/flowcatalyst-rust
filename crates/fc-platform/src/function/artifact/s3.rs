//! Java `S3ArtifactBlobStore`: `FC_FN_ARTIFACT_STORE=s3://bucket[/prefix]`,
//! key `<prefix>/<functionId>/<hex>`. `exists` then `put`: the content at a
//! digest is identical by construction, so `If-None-Match` is not relied on.
//!
//! The client uses the default credential chain (the task role). It skips
//! the SDK's default per-request checksums (Java sets
//! `RequestChecksumCalculation.WHEN_REQUIRED`): the platform's own sha256
//! is the integrity check that matters, and the default wraps `PutObject`
//! in `aws-chunked` framing. It is built on first use, since loading the
//! AWS configuration is asynchronous and startup wiring is not.

use std::path::Path;

use async_trait::async_trait;
use aws_sdk_s3::config::http::HttpResponse;
use aws_sdk_s3::config::{BehaviorVersion, RequestChecksumCalculation, ResponseChecksumValidation};
use aws_sdk_s3::error::SdkError;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;
use tokio::sync::OnceCell;

use super::{keys, ArtifactBlobStore, ArtifactError, ArtifactStream};
use crate::function::Digest;

pub struct S3ArtifactBlobStore {
    client: OnceCell<Client>,
    bucket: String,
    /// `""` when none is configured, else with neither a leading nor a
    /// trailing slash.
    prefix: String,
}

impl std::fmt::Debug for S3ArtifactBlobStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3ArtifactBlobStore")
            .field("bucket", &self.bucket)
            .field("prefix", &self.prefix)
            .finish_non_exhaustive()
    }
}

/// The HTTP status an SDK call failed with, when it got a response.
fn status_of<E>(e: &SdkError<E, HttpResponse>) -> Option<u16> {
    e.raw_response().map(|r| r.status().as_u16())
}

fn transport<E: std::fmt::Debug, R: std::fmt::Debug>(e: SdkError<E, R>) -> ArtifactError {
    ArtifactError::Transport(format!("{e:?}"))
}

impl S3ArtifactBlobStore {
    /// A store whose client is built from the default AWS configuration on
    /// first use.
    pub fn from_environment(bucket: String, prefix: String) -> S3ArtifactBlobStore {
        S3ArtifactBlobStore {
            client: OnceCell::new(),
            bucket,
            prefix,
        }
    }

    /// A store over a given client (tests point one at a fake endpoint).
    pub fn with_client(client: Client, bucket: String, prefix: String) -> S3ArtifactBlobStore {
        S3ArtifactBlobStore {
            client: OnceCell::new_with(Some(client)),
            bucket,
            prefix,
        }
    }

    async fn client(&self) -> &Client {
        self.client
            .get_or_init(|| async {
                let shared = aws_config::load_defaults(BehaviorVersion::latest()).await;
                let config = aws_sdk_s3::config::Builder::from(&shared)
                    .request_checksum_calculation(RequestChecksumCalculation::WhenRequired)
                    .response_checksum_validation(ResponseChecksumValidation::WhenRequired)
                    .build();
                Client::from_conf(config)
            })
            .await
    }

    /// `<prefix>/<functionId>/`, or `<functionId>/` with no prefix.
    fn object_prefix(&self, function_id: &str) -> Result<String, ArtifactError> {
        let id = keys::validate_function_id(function_id)?;
        Ok(if self.prefix.is_empty() {
            format!("{id}/")
        } else {
            format!("{}/{id}/", self.prefix)
        })
    }

    fn object_key(&self, function_id: &str, digest: &Digest) -> Result<String, ArtifactError> {
        let k = keys::of(function_id, digest)?;
        Ok(format!("{}{}", self.object_prefix(k.function_id)?, k.hex))
    }

    /// `HEAD`: the object's length, or `None` when absent.
    async fn head(&self, key: &str) -> Result<Option<u64>, ArtifactError> {
        match self
            .client()
            .await
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
        {
            Ok(out) => Ok(Some(out.content_length().unwrap_or(0).max(0) as u64)),
            Err(e) if status_of(&e) == Some(404) => Ok(None),
            Err(e) => Err(transport(e)),
        }
    }
}

#[async_trait]
impl ArtifactBlobStore for S3ArtifactBlobStore {
    async fn put(
        &self,
        function_id: &str,
        digest: &Digest,
        file: &Path,
    ) -> Result<(), ArtifactError> {
        let key = self.object_key(function_id, digest)?;
        if self.head(&key).await?.is_some() {
            return Ok(());
        }
        let body = ByteStream::from_path(file)
            .await
            .map_err(|e| ArtifactError::Transport(e.to_string()))?;
        self.client()
            .await
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(body)
            .send()
            .await
            .map_err(transport)?;
        Ok(())
    }

    async fn exists(&self, function_id: &str, digest: &Digest) -> Result<bool, ArtifactError> {
        let key = self.object_key(function_id, digest)?;
        Ok(self.head(&key).await?.is_some())
    }

    async fn open(
        &self,
        function_id: &str,
        digest: &Digest,
    ) -> Result<ArtifactStream, ArtifactError> {
        let key = self.object_key(function_id, digest)?;
        match self
            .client()
            .await
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
        {
            Ok(out) => Ok(Box::pin(out.body.into_async_read())),
            Err(e) if status_of(&e) == Some(404) => Err(ArtifactError::NotFound),
            Err(e) => Err(transport(e)),
        }
    }

    async fn size(&self, function_id: &str, digest: &Digest) -> Result<u64, ArtifactError> {
        let key = self.object_key(function_id, digest)?;
        self.head(&key).await?.ok_or(ArtifactError::NotFound)
    }

    async fn delete_all(&self, function_id: &str) -> Result<(), ArtifactError> {
        let prefix = self.object_prefix(function_id)?;
        let client = self.client().await;
        let mut token: Option<String> = None;
        loop {
            let page = client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(&prefix)
                .set_continuation_token(token.take())
                .send()
                .await
                .map_err(transport)?;
            for object in page.contents() {
                if let Some(key) = object.key() {
                    client
                        .delete_object()
                        .bucket(&self.bucket)
                        .key(key)
                        .send()
                        .await
                        .map_err(transport)?;
                }
            }
            match (page.is_truncated(), page.next_continuation_token()) {
                (Some(true), Some(next)) => token = Some(next.to_string()),
                _ => return Ok(()),
            }
        }
    }
}

/// Java `S3ArtifactBlobStoreTest` (U11): the real SDK client, path-style,
/// against an in-process fake of the S3 REST API (Java `FakeS3`).
#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    use axum::body::{Body, Bytes};
    use axum::extract::{Request, State};
    use axum::http::{Method, StatusCode};
    use axum::response::{IntoResponse, Response};
    use sha2::{Digest as _, Sha256};
    use tokio::io::AsyncReadExt;

    use super::*;

    type Objects = Arc<Mutex<BTreeMap<String, Vec<u8>>>>;

    /// Just enough of PutObject, HeadObject, GetObject, DeleteObject and
    /// ListObjectsV2, keyed by the raw request path `/{bucket}/{key…}`.
    struct FakeS3 {
        port: u16,
        objects: Objects,
    }

    impl FakeS3 {
        async fn start() -> FakeS3 {
            let objects: Objects = Arc::default();
            let app = axum::Router::new()
                .fallback(handle)
                .with_state(objects.clone());
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            FakeS3 { port, objects }
        }

        fn client(&self) -> Client {
            let config = aws_sdk_s3::Config::builder()
                .behavior_version(BehaviorVersion::latest())
                .endpoint_url(format!("http://127.0.0.1:{}", self.port))
                .force_path_style(true)
                .region(aws_sdk_s3::config::Region::new("us-east-1"))
                .credentials_provider(aws_sdk_s3::config::Credentials::new(
                    "test", "test", None, None, "test",
                ))
                .request_checksum_calculation(RequestChecksumCalculation::WhenRequired)
                .response_checksum_validation(ResponseChecksumValidation::WhenRequired)
                .build();
            Client::from_conf(config)
        }

        fn store(&self, prefix: &str) -> S3ArtifactBlobStore {
            S3ArtifactBlobStore::with_client(self.client(), "my-bucket".into(), prefix.into())
        }

        /// Straight from the fake's map, independent of the store's own
        /// key building.
        fn has(&self, path: &str) -> bool {
            self.objects.lock().unwrap().contains_key(path)
        }
    }

    async fn handle(State(objects): State<Objects>, req: Request) -> Response {
        let method = req.method().clone();
        let path = req.uri().path().to_string();
        let query = req.uri().query().unwrap_or("").to_string();
        let bucket_only = !path.trim_matches('/').contains('/');
        match method {
            Method::PUT => {
                let body: Bytes = axum::body::to_bytes(req.into_body(), usize::MAX)
                    .await
                    .unwrap();
                objects.lock().unwrap().insert(path, body.to_vec());
                (StatusCode::OK, [("ETag", "\"fake\"")]).into_response()
            }
            Method::HEAD => match objects.lock().unwrap().get(&path) {
                Some(body) => Response::builder()
                    .status(200)
                    .header("Content-Length", body.len())
                    .body(Body::empty())
                    .unwrap(),
                None => StatusCode::NOT_FOUND.into_response(),
            },
            Method::GET if bucket_only => list(&objects, &path, &query),
            Method::GET => match objects.lock().unwrap().get(&path) {
                Some(body) => (StatusCode::OK, body.clone()).into_response(),
                None => xml(
                    StatusCode::NOT_FOUND,
                    format!(
                        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Error><Code>NoSuchKey</Code>\
                         <Message>The specified key does not exist.</Message><Key>{path}</Key>\
                         <RequestId>fake-request-id</RequestId></Error>"
                    ),
                ),
            },
            Method::DELETE => {
                objects.lock().unwrap().remove(&path);
                StatusCode::NO_CONTENT.into_response()
            }
            _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
        }
    }

    fn list(objects: &Objects, bucket_path: &str, query: &str) -> Response {
        let prefix = query
            .split('&')
            .find_map(|pair| pair.strip_prefix("prefix="))
            .map(|v| urlencoding::decode(v).unwrap().into_owned())
            .unwrap_or_default();
        let bucket_prefix = format!("{}/", bucket_path.trim_end_matches('/'));
        let full = format!("{bucket_prefix}{prefix}");
        let objects = objects.lock().unwrap();
        let keys: Vec<(&String, usize)> = objects
            .iter()
            .filter(|(k, _)| k.starts_with(&full))
            .map(|(k, v)| (k, v.len()))
            .collect();
        let mut out = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
             <ListBucketResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
             <Name>{}</Name><Prefix>{prefix}</Prefix><KeyCount>{}</KeyCount>\
             <MaxKeys>1000</MaxKeys><IsTruncated>false</IsTruncated>",
            bucket_path.trim_start_matches('/'),
            keys.len()
        );
        for (key, size) in keys {
            out.push_str(&format!(
                "<Contents><Key>{}</Key><LastModified>2026-01-01T00:00:00.000Z</LastModified>\
                 <ETag>&quot;fake&quot;</ETag><Size>{size}</Size>\
                 <StorageClass>STANDARD</StorageClass></Contents>",
                &key[bucket_prefix.len()..]
            ));
        }
        out.push_str("</ListBucketResult>");
        xml(StatusCode::OK, out)
    }

    fn xml(status: StatusCode, body: String) -> Response {
        (status, [("Content-Type", "application/xml")], body).into_response()
    }

    fn digest_of(bytes: &[u8]) -> Digest {
        Digest::from_sha256(&Sha256::digest(bytes).into())
    }

    fn source(text: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "fc-s3-src-{}.bin",
            crate::shared::tsid::generate_untyped()
        ));
        std::fs::write(&p, text).unwrap();
        p
    }

    async fn read(store: &S3ArtifactBlobStore, id: &str, d: &Digest) -> Vec<u8> {
        let mut out = Vec::new();
        store
            .open(id, d)
            .await
            .unwrap()
            .read_to_end(&mut out)
            .await
            .unwrap();
        out
    }

    #[tokio::test]
    async fn put_exists_open_size_round_trip_with_no_prefix() {
        let fake = FakeS3::start().await;
        let store = fake.store("");
        let d = digest_of(b"hello-s3");
        assert!(!store.exists("fn1", &d).await.unwrap());
        store.put("fn1", &d, &source("hello-s3")).await.unwrap();
        assert!(store.exists("fn1", &d).await.unwrap());
        assert_eq!(store.size("fn1", &d).await.unwrap(), 8);
        assert_eq!(read(&store, "fn1", &d).await, b"hello-s3");
        assert!(fake.has(&format!("/my-bucket/fn1/{}", d.hex())));
    }

    #[tokio::test]
    async fn key_layout_includes_the_configured_prefix() {
        let fake = FakeS3::start().await;
        let store = fake.store("fn-artifacts");
        let d = digest_of(b"with-prefix");
        store.put("fn1", &d, &source("with-prefix")).await.unwrap();
        assert!(fake.has(&format!("/my-bucket/fn-artifacts/fn1/{}", d.hex())));
        assert!(store.exists("fn1", &d).await.unwrap());
    }

    #[tokio::test]
    async fn put_is_idempotent_an_existing_object_is_never_overwritten() {
        let fake = FakeS3::start().await;
        let store = fake.store("");
        let d = digest_of(b"first");
        store.put("fn1", &d, &source("first")).await.unwrap();
        store
            .put("fn1", &d, &source("second-never-uploaded"))
            .await
            .unwrap();
        assert_eq!(read(&store, "fn1", &d).await, b"first");
    }

    #[tokio::test]
    async fn open_and_size_of_a_missing_object_are_not_found() {
        let fake = FakeS3::start().await;
        let store = fake.store("");
        let d = Digest::parse(&format!("sha256:{}", "0".repeat(64))).unwrap();
        assert!(matches!(
            store.open("fn1", &d).await,
            Err(ArtifactError::NotFound)
        ));
        assert_eq!(
            store.size("fn1", &d).await.unwrap_err(),
            ArtifactError::NotFound
        );
    }

    #[tokio::test]
    async fn delete_all_removes_every_object_under_the_function_with_and_without_a_prefix() {
        let fake = FakeS3::start().await;
        for prefix in ["", "fn-artifacts"] {
            let store = fake.store(prefix);
            let a = digest_of(b"a");
            let b = digest_of(format!("b-{prefix}").as_bytes());
            store.put("fnDel", &a, &source("a")).await.unwrap();
            store
                .put("fnDel", &b, &source(&format!("b-{prefix}")))
                .await
                .unwrap();
            store.put("fnOther", &a, &source("a")).await.unwrap();
            store.delete_all("fnDel").await.unwrap();
            assert!(!store.exists("fnDel", &a).await.unwrap(), "{prefix:?}");
            assert!(!store.exists("fnDel", &b).await.unwrap(), "{prefix:?}");
            assert!(store.exists("fnOther", &a).await.unwrap(), "{prefix:?}");
        }
    }

    #[tokio::test]
    async fn the_store_validates_its_keys_before_any_request() {
        let fake = FakeS3::start().await;
        let store = fake.store("");
        let d = digest_of(b"x");
        assert!(matches!(
            store.put("../x", &d, &source("x")).await,
            Err(ArtifactError::BadRef(_))
        ));
        assert!(fake.objects.lock().unwrap().is_empty());
    }
}
