//! Serving the SPA (Go `frontend.Handler`, frontend/handler.go): the
//! answer to every request no platform route claims, for the SPA built into
//! fc-dev and for a `static_dir` on disk alike.
//!
//! 1. A path under an API prefix ([`is_api_path`]): 404 with the API's JSON
//!    envelope, never the SPA shell (a client calling a mistyped or removed
//!    route must not get 200 and HTML).
//! 2. A method other than GET and HEAD: 405 (`Allow: GET, HEAD`).
//! 3. An existing file: served with its MIME type and a content-hash `ETag`
//!    (304 on `If-None-Match`). Files under `/assets/` (Vite's hashed names)
//!    are `public, max-age=31536000, immutable`; any other file `no-cache`.
//! 4. A missing file under `/assets/`, or with a static-asset extension
//!    ([`ASSET_EXTENSIONS`]): 404, `no-store` (a stale hashed chunk must fail
//!    as a 404, not load the shell as JavaScript, and a deploy in progress
//!    must not leave a cached 404).
//! 5. Anything else (an extension-less client route): `index.html`,
//!    `no-cache` with its `ETag`, so the client router takes over and a new
//!    deploy's shell is picked up on the next load.
//!
//! Every response carries [`set_security_headers`]: nosniff,
//! `Referrer-Policy: same-origin`, framing denial and a CSP limited to
//! `frame-ancestors`, `object-src` and `base-uri` (no script or style
//! policy: the SPA has not been run under one; Go F16).

use std::borrow::Cow;
use std::path::PathBuf;

use axum::body::Body;
use axum::http::header::{
    ALLOW, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_SECURITY_POLICY as CSP, CONTENT_TYPE, ETAG,
    IF_NONE_MATCH, REFERRER_POLICY, X_CONTENT_TYPE_OPTIONS, X_FRAME_OPTIONS,
};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Json;
use mime_guess::mime;
use sha2::{Digest, Sha256};
use tokio::fs;

/// The first path segments the platform's APIs own (Go `apiPrefixes`). The
/// SPA has no client routes under them.
pub const API_PREFIXES: &[&str] = &["api", "bff", "oauth", ".well-known", "control", "q"];

/// The file types a missing path is a 404 for rather than a client route (Go
/// `assetExtensions`). An allow-list, not "has a dot": client routes carry
/// identifiers that may contain dots.
pub const ASSET_EXTENSIONS: &[&str] = &[
    "js",
    "mjs",
    "cjs",
    "css",
    "map",
    "json",
    "webmanifest",
    "txt",
    "xml",
    "wasm",
    "html",
    "htm",
    "png",
    "jpg",
    "jpeg",
    "gif",
    "svg",
    "ico",
    "webp",
    "avif",
    "bmp",
    "woff",
    "woff2",
    "ttf",
    "otf",
    "eot",
];

/// Restricts framing, plugins and `<base>` only (Go
/// `contentSecurityPolicy`). No script or style policy: PrimeVue injects
/// `<style>` elements at runtime and the Swagger UI chunk uses the
/// `Function` constructor.
pub const CONTENT_SECURITY_POLICY: &str =
    "frame-ancestors 'none'; object-src 'none'; base-uri 'self'";

/// `Cache-Control` of hashed `/assets/*`.
pub const IMMUTABLE: &str = "public, max-age=31536000, immutable";
/// `Cache-Control` of the shell and every other existing file: cacheable,
/// revalidated with the `ETag` on every use.
pub const REVALIDATE: &str = "no-cache";
/// `Cache-Control` of every error answer.
pub const NO_STORE: &str = "no-store";

/// Whether `path` (a URL path) lies under one of [`API_PREFIXES`], matched
/// by whole first segment (`/apiary` is not `/api`). Go `IsAPIPath`.
pub fn is_api_path(path: &str) -> bool {
    let first = path
        .strip_prefix('/')
        .unwrap_or(path)
        .split('/')
        .next()
        .unwrap_or("");
    API_PREFIXES.contains(&first)
}

/// A file's bytes and their SHA-256.
pub type SpaFile = (Cow<'static, [u8]>, [u8; 32]);

/// Where the SPA's files come from.
#[derive(Clone)]
pub enum SpaFiles {
    /// A built SPA on disk (`static_dir`, fc-dev's `FC_STATIC_DIR`), read on
    /// each request so a rebuild is served at once.
    Dir(PathBuf),
    /// Files compiled into the binary (fc-dev): a lookup by relative path
    /// giving the bytes and their SHA-256.
    Embedded(fn(&str) -> Option<SpaFile>),
}

impl SpaFiles {
    /// The file at `rel` (relative, cleaned, never empty) and its SHA-256,
    /// or `None` when there is no such file (a directory included).
    async fn get(&self, rel: &str) -> Option<SpaFile> {
        match self {
            Self::Dir(root) => {
                let bytes = fs::read(root.join(rel)).await.ok()?;
                let digest: [u8; 32] = Sha256::digest(&bytes).into();
                Some((Cow::Owned(bytes), digest))
            }
            Self::Embedded(lookup) => lookup(rel),
        }
    }
}

/// Set the headers every SPA response carries (Go `setSecurityHeaders`).
pub fn set_security_headers(headers: &mut HeaderMap) {
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    headers.insert(X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(CSP, HeaderValue::from_static(CONTENT_SECURITY_POLICY));
}

/// The answer to `method` `uri` (the rules in the module header).
pub async fn respond(
    files: &SpaFiles,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
) -> Response {
    let mut response = answer(files, method, uri, headers).await;
    set_security_headers(response.headers_mut());
    response
}

async fn answer(files: &SpaFiles, method: &Method, uri: &Uri, headers: &HeaderMap) -> Response {
    // Go matches the decoded path (`r.URL.Path`).
    let decoded = urlencoding::decode(uri.path()).map_or_else(
        |_| Cow::Borrowed(uri.path()),
        |p| Cow::Owned(p.into_owned()),
    );
    if is_api_path(&decoded) {
        return api_not_found(method, &decoded);
    }
    if method != Method::GET && method != Method::HEAD {
        let mut response = plain_status(StatusCode::METHOD_NOT_ALLOWED);
        response
            .headers_mut()
            .insert(ALLOW, HeaderValue::from_static("GET, HEAD"));
        return response;
    }
    let head = method == Method::HEAD;

    let rel = clean(&decoded);
    if rel.is_empty() || rel == "index.html" {
        return shell(files, headers, head).await;
    }
    if !rel.contains('\0') {
        if let Some((bytes, digest)) = files.get(&rel).await {
            let cache = if rel.starts_with("assets/") {
                IMMUTABLE
            } else {
                REVALIDATE
            };
            return file_response(&rel, bytes, &digest, cache, headers, head);
        }
    }
    let extension = rel
        .rsplit_once('/')
        .map_or(rel.as_str(), |(_, name)| name)
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase());
    if rel.starts_with("assets/")
        || rel == "assets"
        || extension.is_some_and(|ext| ASSET_EXTENSIONS.contains(&ext.as_str()))
    {
        return plain_status(StatusCode::NOT_FOUND);
    }
    shell(files, headers, head).await
}

/// `index.html`, revalidated with its `ETag`.
async fn shell(files: &SpaFiles, headers: &HeaderMap, head: bool) -> Response {
    match files.get("index.html").await {
        Some((bytes, digest)) => {
            file_response("index.html", bytes, &digest, REVALIDATE, headers, head)
        }
        // Go `missingFrontend`: say the SPA was not built rather than 404
        // as if the page did not exist.
        None => (
            StatusCode::NOT_FOUND,
            [(CACHE_CONTROL, HeaderValue::from_static(NO_STORE))],
            "Frontend not built into this binary.\n\
             Run `cd frontend && pnpm install && pnpm build` and rebuild the binary.\n",
        )
            .into_response(),
    }
}

/// Go's `path.Clean("/" + p)` without the leading slash: `.` and `..`
/// resolved against the root (so `..` never climbs out of it), empty
/// segments dropped.
fn clean(path: &str) -> String {
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            s => segments.push(s),
        }
    }
    segments.join("/")
}

/// The ETag Go gives a file: the first 16 bytes of its SHA-256 in hex,
/// quoted.
pub fn etag(digest: &[u8; 32]) -> String {
    format!("\"{}\"", hex::encode(&digest[..16]))
}

/// Whether `If-None-Match` names `etag` (weak comparison; `*` matches),
/// Go `checkIfNoneMatch`.
fn none_match(headers: &HeaderMap, etag: &str) -> bool {
    let strip = |t: &str| t.trim().trim_start_matches("W/").to_string();
    let want = strip(etag);
    headers
        .get_all(IF_NONE_MATCH)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .any(|t| t.trim() == "*" || strip(t) == want)
}

fn file_response(
    name: &str,
    bytes: Cow<'static, [u8]>,
    digest: &[u8; 32],
    cache_control: &'static str,
    request: &HeaderMap,
    head: bool,
) -> Response {
    let tag = etag(digest);
    let mut headers = HeaderMap::new();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static(cache_control));
    if let Ok(value) = HeaderValue::from_str(&tag) {
        headers.insert(ETAG, value);
    }
    if none_match(request, &tag) {
        return (StatusCode::NOT_MODIFIED, headers).into_response();
    }
    headers.insert(CONTENT_TYPE, content_type(name));
    if head {
        let mut response = (headers, Body::empty()).into_response();
        if let Ok(len) = HeaderValue::from_str(&bytes.len().to_string()) {
            response.headers_mut().insert(CONTENT_LENGTH, len);
        }
        return response;
    }
    (headers, Body::from(bytes)).into_response()
}

/// The file's MIME type, with `charset=utf-8` on text types as Go's
/// `mime.TypeByExtension` gives them.
fn content_type(name: &str) -> HeaderValue {
    let guessed = mime_guess::from_path(name).first_or_octet_stream();
    let value =
        if guessed.type_() == mime::TEXT || guessed.essence_str() == "application/javascript" {
            format!("{}; charset=utf-8", guessed.essence_str())
        } else {
            guessed.essence_str().to_string()
        };
    HeaderValue::from_str(&value).unwrap_or(HeaderValue::from_static("application/octet-stream"))
}

/// `text/plain` status line, `no-store` (Go `plainStatus`).
fn plain_status(status: StatusCode) -> Response {
    let text = format!("{}\n", status.canonical_reason().unwrap_or(""));
    (
        status,
        [
            (
                CONTENT_TYPE,
                HeaderValue::from_static("text/plain; charset=utf-8"),
            ),
            (CACHE_CONTROL, HeaderValue::from_static(NO_STORE)),
        ],
        text,
    )
        .into_response()
}

/// An unknown API path: 404 in the API's JSON envelope (Go `apiNotFound`).
fn api_not_found(method: &Method, path: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        [(CACHE_CONTROL, HeaderValue::from_static(NO_STORE))],
        Json(serde_json::json!({
            "error": "NOT_FOUND",
            "message": format!("No route for {method} {path}"),
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_prefixes_match_by_whole_segment() {
        for p in [
            "/api",
            "/api/",
            "/api/x",
            "/bff/x",
            "/oauth/x",
            "/.well-known/x",
            "/control/x",
            "/q/x",
        ] {
            assert!(is_api_path(p), "{p}");
        }
        for p in [
            "/",
            "/apiary",
            "/bffs",
            "/clients/api",
            "/queue",
            "/auth/login",
        ] {
            assert!(!is_api_path(p), "{p}");
        }
    }

    #[test]
    fn clean_never_climbs_out_of_the_root() {
        assert_eq!(clean("/"), "");
        assert_eq!(clean("/../../etc/passwd"), "etc/passwd");
        assert_eq!(clean("/assets/../index.html"), "index.html");
        assert_eq!(clean("//assets/./a.js"), "assets/a.js");
    }

    #[test]
    fn if_none_match_compares_weakly() {
        let tag = "\"abc\"";
        let mut h = HeaderMap::new();
        assert!(!none_match(&h, tag));
        h.insert(IF_NONE_MATCH, HeaderValue::from_static("\"x\", W/\"abc\""));
        assert!(none_match(&h, tag));
        h.insert(IF_NONE_MATCH, HeaderValue::from_static("*"));
        assert!(none_match(&h, tag));
        h.insert(IF_NONE_MATCH, HeaderValue::from_static("\"abd\""));
        assert!(!none_match(&h, tag));
    }
}
