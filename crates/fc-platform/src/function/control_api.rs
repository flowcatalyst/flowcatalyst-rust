//! The surface a function host calls (Java
//! `function/api/FunctionControlApi.java`, spec `function-api.md` §6,
//! `function-context.md` §3, `function-artifact-upload.md` §4):
//!
//! | Method | Path | Status |
//! |---|---|---|
//! | GET | `/control/functions/desired-state?pool=` | 200 / 304 |
//! | POST | `/control/functions/heartbeat` | 204 |
//! | POST | `/control/functions/events` | 201 |
//! | GET | `/control/functions/artifacts/{versionId}` | 200 (a stream) |
//!
//! **The gate, on all four** (Java `gate`, `FunctionControlApi.java:473-482`):
//! 401 without a credential (the platform's bearer extractor; the host
//! authenticates with OAuth `client_credentials`), then anchor scope (`403
//! ANCHOR_REQUIRED`) and `platform:function:host:control` (`403
//! PERMISSION_REQUIRED`), which only the built-in `function-host` role
//! grants. The wire formats are Java's, so the Java host runs against these
//! routes as the Rust host does.
//!
//! **What is not a use case here, and why** (CLAUDE.md's platform
//! infrastructure exceptions, as Java):
//! - The heartbeat's host upsert and stale-host purge write `fn_hosts`
//!   through its repository with no event and no audit: a heartbeat is
//!   telemetry every 15 s per host, and an event per beat would swamp the
//!   event log (Java: "the host row alone, no event, no audit").
//! - Emitted events are ingested exactly as `POST /api/events/batch` ingests
//!   them (the same repository insert, no unit of work): wrapping an ingest
//!   in a use case would emit an event about each event.
//!
//! What a heartbeat causes (a version becoming `READY`) is a use case,
//! [`MarkVersionReadyUseCase`], with its `version:ready` event and audit row.

use std::collections::HashSet;
use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use serde::Deserialize;
use serde_json::value::RawValue;

use super::api::{query_param, QueryParams};
use super::artifact::{self, ArtifactBlobStore, ArtifactError, PlatformArtifactRef};
use super::desired_state::{etag, etag_matches, DesiredStateBuilder};
use super::entity::{
    Function, FunctionHost, FunctionStatus, FunctionVersion, HostState, LoadState, LoadedVersion,
};
use super::host_repository::FunctionHostRepository;
use super::json::JsonNode;
use super::operations::access::resource_not_found;
use super::operations::mark_ready::{
    MarkVersionReadyCommand, MarkVersionReadyUseCase, VERSION_NOT_PUBLISHED,
};
use super::repository::FunctionRepository;
use super::version_repository::FunctionVersionRepository;
use super::wire::parse_body;
use super::{java_is_blank, parse_address, DnsLabel, FunctionAddress, FunctionOwner};
use crate::event::entity::Event;
use crate::event::repository::EventRepository;
use crate::event_type::entity::EventTypeStatus;
use crate::event_type::repository::EventTypeRepository;
use crate::permissions::function::FUNCTION_HOST_CONTROL;
use crate::shared::authorization_service::{checks, AuthContext};
use crate::shared::batch_api::{BatchResponse, BatchResultItem};
use crate::shared::error::PlatformError;
use crate::shared::middleware::Authenticated;
use crate::usecase::{ExecutionContext, PgUnitOfWork, UseCase, UseCaseError};
use crate::ApplicationRepository;

/// 1-100 events per emit call.
pub const MAX_EMIT_BATCH: usize = 100;
/// An event's `data` is at most 256 KiB, as Jackson writes it.
pub const MAX_EMIT_DATA_BYTES: usize = 256 * 1024;
/// A `FAILED` entry's error is kept to 1000 characters.
const MAX_ERROR_LENGTH: usize = 1000;

#[derive(Clone)]
pub struct FunctionControlState {
    pub desired: Arc<DesiredStateBuilder>,
    pub functions: Arc<FunctionRepository>,
    pub versions: Arc<FunctionVersionRepository>,
    pub hosts: Arc<FunctionHostRepository>,
    pub applications: Arc<ApplicationRepository>,
    pub event_types: Arc<EventTypeRepository>,
    pub events: Arc<EventRepository>,
    /// `FC_FN_ARTIFACT_STORE`; `None` when unset.
    pub artifacts: Option<Arc<dyn ArtifactBlobStore>>,
    pub unit_of_work: Arc<PgUnitOfWork>,
}

/// Anchor, then the host-control permission (Java `gate`); the 401 for no
/// credential is the extractor's.
fn gate(auth: &AuthContext) -> Result<(), PlatformError> {
    checks::require_anchor_scope(auth)?;
    checks::require_permission(auth, FUNCTION_HOST_CONTROL)
}

fn parse_pool(raw: Option<&str>) -> Result<DnsLabel, UseCaseError> {
    DnsLabel::parse("pool", raw.unwrap_or(""))
        .map_err(|_| UseCaseError::validation("POOL_INVALID", "pool must be a DNS label"))
}

// ── GET /control/functions/desired-state ────────────────────────────────────

/// The pool's document, serialised once: the `ETag` is the sha256 of
/// exactly the bytes a 200 returns. A matching `If-None-Match` is a 304
/// with the `ETag` and no body.
pub async fn desired_state(
    State(state): State<FunctionControlState>,
    auth: Authenticated,
    Query(params): Query<QueryParams>,
    headers: HeaderMap,
) -> Result<Response, PlatformError> {
    gate(&auth.0)?;
    let pool = parse_pool(query_param(&params, "pool"))?;
    let document = state.desired.build(&pool, Utc::now()).await?;
    let body = document.to_bytes();
    let tag = etag(&body);
    let etag_header = HeaderValue::from_str(&tag).expect("a quoted hex digest is a header value");
    let if_none_match = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok());
    if etag_matches(if_none_match, &tag) {
        return Ok((StatusCode::NOT_MODIFIED, [(header::ETAG, etag_header)]).into_response());
    }
    Ok((
        StatusCode::OK,
        [
            (header::ETAG, etag_header),
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            ),
        ],
        body,
    )
        .into_response())
}

// ── POST /control/functions/heartbeat ───────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HeartbeatRequest {
    host_id: Option<String>,
    pool: Option<String>,
    state: Option<String>,
    loaded: Option<Vec<Option<LoadedEntry>>>,
}

#[derive(Debug, Deserialize)]
struct LoadedEntry {
    address: Option<String>,
    version: Option<i64>,
    state: Option<String>,
    error: Option<String>,
}

/// `hostId`: 1-100 characters of `[A-Za-z0-9._:-]`.
fn valid_host_id(id: &str) -> bool {
    (1..=100).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
}

fn loaded_invalid(index: usize, message: impl std::fmt::Display) -> UseCaseError {
    UseCaseError::validation("LOADED_INVALID", format!("loaded[{index}]: {message}"))
}

/// Strict, unlike the stored reader: an unreadable entry is `400
/// LOADED_INVALID` naming its index.
fn parse_loaded_entry(
    entry: Option<&LoadedEntry>,
    index: usize,
) -> Result<LoadedVersion, UseCaseError> {
    let entry = entry.ok_or_else(|| loaded_invalid(index, "entry is required"))?;
    let address = parse_address(entry.address.as_deref().unwrap_or(""))
        .map_err(|e| loaded_invalid(index, format!("address: {}", e.message())))?;
    let version = entry
        .version
        .filter(|v| *v > 0)
        .and_then(|v| i32::try_from(v).ok())
        .ok_or_else(|| loaded_invalid(index, "version must be a positive integer"))?;
    let state = match entry.state.as_deref() {
        None => return Err(loaded_invalid(index, "state is required")),
        Some("REGISTERED") => LoadState::Registered,
        Some("LOADED") => LoadState::Loaded,
        Some("FAILED") => LoadState::Failed(truncate_utf16(
            entry.error.as_deref().unwrap_or(""),
            MAX_ERROR_LENGTH,
        )),
        Some(_) => {
            return Err(loaded_invalid(
                index,
                "state must be REGISTERED, LOADED, or FAILED",
            ))
        }
    };
    Ok(LoadedVersion {
        address,
        version,
        state,
    })
}

/// Java's `substring(0, max)` on a UTF-16 string, never splitting a
/// character.
fn truncate_utf16(s: &str, max: usize) -> String {
    let mut units = 0;
    let mut end = s.len();
    for (i, c) in s.char_indices() {
        units += c.len_utf16();
        if units > max {
            end = i;
            break;
        }
    }
    s[..end].to_string()
}

/// Upserts the host and purges hosts stale for over a day, in one
/// transaction (no event, no audit); then marks `READY` every version the
/// host reports `REGISTERED` or `LOADED` that is still `PUBLISHED`. A lost
/// race to mark one ready (`VERSION_NOT_PUBLISHED`) is ignored.
pub async fn heartbeat(
    State(state): State<FunctionControlState>,
    auth: Authenticated,
    body: Bytes,
) -> Result<StatusCode, PlatformError> {
    gate(&auth.0)?;
    let req: HeartbeatRequest = parse_body(&body)?;
    let host_id = req.host_id.filter(|id| valid_host_id(id)).ok_or_else(|| {
        UseCaseError::validation(
            "HOST_ID_INVALID",
            "hostId must be 1-100 characters of [A-Za-z0-9._:-]",
        )
    })?;
    let pool = parse_pool(req.pool.as_deref())?;
    let host_state = match req.state.as_deref() {
        Some("ACTIVE") => HostState::Active,
        Some("DRAINING") => HostState::Draining,
        _ => {
            return Err(UseCaseError::validation(
                "HOST_STATE_INVALID",
                "state must be ACTIVE or DRAINING",
            )
            .into())
        }
    };
    let raw_loaded = req.loaded.unwrap_or_default();
    let loaded = raw_loaded
        .iter()
        .enumerate()
        .map(|(i, e)| parse_loaded_entry(e.as_ref(), i))
        .collect::<Result<Vec<_>, _>>()?;

    let now = Utc::now();
    let host = match state.hosts.find_by_id(&host_id).await? {
        Some(existing) => existing.heartbeat(host_state, loaded.clone(), now),
        None => FunctionHost::register(&host_id, pool.value(), now).heartbeat(
            host_state,
            loaded.clone(),
            now,
        ),
    };
    let purged = state
        .hosts
        .heartbeat(&host, now - FunctionHost::purge_after())
        .await?;
    if purged > 0 {
        tracing::info!(count = purged, "function hosts purged");
    }

    mark_ready(&state, &auth.0, &host_id, &loaded).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Spec §6.2 step 2, in report order: only an `ok` entry (never `FAILED`)
/// whose function exists and whose version is still `PUBLISHED`. The
/// functions and versions are read in one query each; the pre-check keeps
/// the routine case (a host re-reporting a `READY` version) from ever
/// reaching the use case.
async fn mark_ready(
    state: &FunctionControlState,
    auth: &AuthContext,
    host_id: &str,
    loaded: &[LoadedVersion],
) -> Result<(), PlatformError> {
    let ok: Vec<&LoadedVersion> = loaded.iter().filter(|lv| lv.state.ok()).collect();
    if ok.is_empty() {
        return Ok(());
    }
    let addresses: Vec<FunctionAddress> = ok.iter().map(|lv| lv.address.clone()).collect();
    let functions = state.functions.find_by_addresses(&addresses).await?;
    let function_of = |address: &FunctionAddress| functions.iter().find(|f| &f.address == address);
    let pairs: Vec<(String, i32)> = ok
        .iter()
        .filter_map(|lv| function_of(&lv.address).map(|f| (f.id.clone(), lv.version)))
        .collect();
    let batch = state
        .versions
        .find_batch_by_function_versions(&pairs)
        .await?;

    for lv in ok {
        let Some(f) = function_of(&lv.address) else {
            continue;
        };
        // A corrupt version fails the beat when reached, as Java's
        // single-row read does (after the host row is written).
        if let Some(corrupt) = batch
            .corrupt
            .iter()
            .find(|c| c.function_id == f.id && c.version == lv.version)
        {
            return Err(super::desired_state::corrupt_row(
                &corrupt.version_id,
                &corrupt.cause,
            ));
        }
        let Some(v) = batch
            .versions
            .values()
            .find(|v| v.function_id == f.id && v.version == lv.version)
        else {
            continue;
        };
        if v.state != super::entity::VersionState::Published {
            continue;
        }
        let command = MarkVersionReadyCommand {
            version_id: v.id.clone(),
            host_id: host_id.to_string(),
        };
        let ctx = ExecutionContext::from_auth(auth);
        let (functions, versions) = (state.functions.clone(), state.versions.clone());
        let result = state
            .unit_of_work
            .run(move |scoped| async move {
                MarkVersionReadyUseCase::new(functions, versions, scoped)
                    .run(command, ctx)
                    .await
            })
            .await
            .into_result();
        match result {
            Ok(_) => {}
            Err(e) if e.code() == VERSION_NOT_PUBLISHED => {
                tracing::debug!(
                    version_id = %v.id,
                    host_id,
                    "heartbeat lost a race marking a version ready; ignoring"
                );
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

// ── POST /control/functions/events ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EmitEventsRequest {
    host_id: Option<String>,
    address: Option<String>,
    version: Option<i64>,
    events: Option<Vec<EmitEventItem>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EmitEventItem {
    #[serde(rename = "type")]
    event_type: Option<String>,
    subject: Option<String>,
    dedup_id: Option<String>,
    data: Option<Box<RawValue>>,
    correlation_id: Option<String>,
    causation_id: Option<String>,
    message_group: Option<String>,
}

fn not_served() -> UseCaseError {
    UseCaseError::business_rule(
        "FUNCTION_NOT_SERVED_BY_HOST",
        "this host does not currently serve that function/version",
    )
}

/// Check 1: `hostId` names a host whose last heartbeat is inside the live
/// window (`409 HOST_UNKNOWN`).
async fn require_live_host(
    state: &FunctionControlState,
    host_id: Option<&str>,
) -> Result<FunctionHost, PlatformError> {
    let since = Utc::now() - FunctionHost::live_window();
    let host = match host_id {
        Some(id) => state.hosts.find_by_id(id).await?,
        None => None,
    };
    host.filter(|h| h.last_heartbeat >= since).ok_or_else(|| {
        UseCaseError::business_rule(
            "HOST_UNKNOWN",
            format!(
                "no host named '{}' has a heartbeat inside the live window",
                host_id.unwrap_or("null")
            ),
        )
        .into()
    })
}

/// Check 2: the function exists, is `ACTIVE`, and `version` is its live
/// version or newest published candidate in this host's pool: the exact
/// selection desired state made (`409 FUNCTION_NOT_SERVED_BY_HOST`).
async fn resolve_served(
    state: &FunctionControlState,
    host: &FunctionHost,
    raw_address: Option<&str>,
    version: Option<i64>,
) -> Result<(Function, FunctionVersion), PlatformError> {
    let address = FunctionAddress::parse(raw_address.unwrap_or("")).map_err(|_| not_served())?;
    let f = state
        .functions
        .find_by_address(&address)
        .await?
        .ok_or_else(not_served)?;
    if f.status != FunctionStatus::Active {
        return Err(not_served().into());
    }
    let number = version
        .filter(|v| *v > 0)
        .and_then(|v| i32::try_from(v).ok())
        .ok_or_else(not_served)?;
    let v = state
        .versions
        .find_by_function_and_version(&f.id, number)
        .await?
        .ok_or_else(not_served)?;
    if !state.desired.serves(&f, &v, &host.pool).await? {
        return Err(not_served().into());
    }
    Ok((f, v))
}

/// Four checks in Java's order, each its own code, and nothing written
/// unless every event passes all of them: the host (409), what it serves
/// (409), the batch (400), and ownership (403: each type an existing,
/// unarchived event type of the function's own application). Then one
/// batch insert through the ingest repository, `source =
/// function:<address>`, `clientId` the function's owner (absent for the
/// platform's).
pub async fn emit_events(
    State(state): State<FunctionControlState>,
    auth: Authenticated,
    body: Bytes,
) -> Result<(StatusCode, Json<BatchResponse>), PlatformError> {
    gate(&auth.0)?;
    let req: EmitEventsRequest = parse_body(&body)?;
    let host = require_live_host(&state, req.host_id.as_deref()).await?;
    let (f, _version) = resolve_served(&state, &host, req.address.as_deref(), req.version).await?;

    let items = req.events.unwrap_or_default();
    if items.is_empty() || items.len() > MAX_EMIT_BATCH {
        return Err(UseCaseError::validation(
            "BATCH_SIZE_INVALID",
            "events must carry 1-100 items",
        )
        .into());
    }
    let mut seen = HashSet::new();
    let mut data = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let dedup = item
            .dedup_id
            .as_deref()
            .filter(|d| !java_is_blank(d))
            .ok_or_else(|| {
                UseCaseError::validation(
                    "DEDUP_ID_REQUIRED",
                    format!("events[{i}].dedupId is required"),
                )
            })?;
        if !seen.insert(dedup) {
            return Err(UseCaseError::validation(
                "DEDUP_ID_DUPLICATE",
                format!("events[{i}].dedupId '{dedup}' is duplicated in this batch"),
            )
            .into());
        }
        let invalid = || {
            UseCaseError::validation(
                "EVENT_DATA_INVALID",
                format!("events[{i}].data must be an object"),
            )
        };
        let raw = item.data.as_ref().ok_or_else(invalid)?;
        let node = JsonNode::parse(raw.get()).map_err(|_| invalid())?;
        if !node.is_object() {
            return Err(invalid().into());
        }
        let bytes = node.to_json_string().len();
        if bytes > MAX_EMIT_DATA_BYTES {
            return Err(UseCaseError::validation(
                "EVENT_DATA_TOO_LARGE",
                format!(
                    "events[{i}].data is {bytes} bytes, which exceeds the limit of {MAX_EMIT_DATA_BYTES}"
                ),
            )
            .into());
        }
        let value: serde_json::Value =
            serde_json::from_str(raw.get()).map_err(|e| super::wire::invalid_json(&e))?;
        data.push(value);
    }

    let application_code = state
        .applications
        .find_by_id(&f.application_id)
        .await?
        .map(|a| a.code)
        .ok_or_else(|| {
            PlatformError::internal(format!(
                "function {} names an application that no longer exists",
                f.id
            ))
        })?;
    let codes: Vec<String> = items
        .iter()
        .filter_map(|item| item.event_type.clone())
        .collect();
    let owners = state.event_types.owners_by_codes(&codes).await?;
    for (i, item) in items.iter().enumerate() {
        let known = item.event_type.as_ref().and_then(|t| owners.get(t));
        let owned = known.is_some_and(|(application, status)| {
            *status != EventTypeStatus::Archived && *application == application_code
        });
        if !owned {
            let type_application = known
                .map(|(application, _)| application.as_str())
                .unwrap_or("(unknown event type)");
            return Err(UseCaseError::forbidden(
                "EVENT_TYPE_NOT_OWNED",
                format!(
                    "events[{i}]: event type '{}' is owned by '{type_application}', not by this function's own application '{application_code}'",
                    item.event_type.as_deref().unwrap_or("null")
                ),
            )
            .into());
        }
    }

    let source = format!("function:{}", f.address.render());
    let client_id = match &f.owner {
        FunctionOwner::Platform => None,
        FunctionOwner::Client(id) => Some(id.clone()),
    };
    let events: Vec<Event> = items
        .into_iter()
        .zip(data)
        .map(|(item, data)| {
            let mut event = Event::new(item.event_type.unwrap_or_default(), &source, data);
            event.subject = item.subject;
            event.deduplication_id = item.dedup_id;
            event.correlation_id = item.correlation_id;
            event.causation_id = item.causation_id;
            event.message_group = item.message_group;
            event.client_id = client_id.clone();
            event
        })
        .collect();
    state.events.insert_many(&events).await?;
    Ok((
        StatusCode::CREATED,
        Json(BatchResponse {
            results: events
                .iter()
                .map(|e| BatchResultItem {
                    id: e.id.clone(),
                    status: "SUCCESS".to_string(),
                })
                .collect(),
        }),
    ))
}

// ── GET /control/functions/artifacts/{versionId} ────────────────────────────

/// The artifact of the version desired state told the host to run,
/// streamed from the blob store: memory never scales with the artifact.
/// `404 FunctionVersion_NOT_FOUND` for an unknown version and for one whose
/// artifact is not `platform://`. No `Digest` header: the host knows the
/// digest from desired state and recomputes it.
pub async fn download_artifact(
    State(state): State<FunctionControlState>,
    auth: Authenticated,
    Path(version_id): Path<String>,
) -> Result<Response, PlatformError> {
    gate(&auth.0)?;
    let store = state
        .artifacts
        .clone()
        .ok_or_else(artifact::store_not_configured)?;
    let not_found = || resource_not_found("FunctionVersion", &version_id);
    let v = state
        .versions
        .find_by_id(&version_id)
        .await?
        .ok_or_else(not_found)?;
    let reference = PlatformArtifactRef::parse(&v.artifact_ref).ok_or_else(not_found)?;
    let opened = async {
        let size = store.size(&reference.function_id, &v.digest).await?;
        let stream = store.open(&reference.function_id, &v.digest).await?;
        Ok::<_, ArtifactError>((size, stream))
    }
    .await;
    let (size, stream) = match opened {
        Ok(opened) => opened,
        Err(ArtifactError::NotFound) => return Err(not_found().into()),
        Err(e) => {
            tracing::error!(version_id = %version_id, error = %e, "reading a function artifact failed");
            return Err(UseCaseError::internal(
                "ARTIFACT_STORE_ERROR",
                "reading the artifact failed",
            )
            .into());
        }
    };
    Ok((
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/octet-stream"),
            ),
            (header::CONTENT_LENGTH, HeaderValue::from(size)),
        ],
        Body::from_stream(tokio_util::io::ReaderStream::new(stream)),
    )
        .into_response())
}

/// The four routes. Not in the OpenAPI document: Java keeps the control
/// plane out of its lockfile too, naming it in `parity/surface.json`.
pub fn function_control_router(state: FunctionControlState) -> Router {
    Router::new()
        .route("/control/functions/desired-state", get(desired_state))
        .route("/control/functions/heartbeat", post(heartbeat))
        .route("/control/functions/events", post(emit_events))
        .route(
            "/control/functions/artifacts/{version_id}",
            get(download_artifact),
        )
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_ids_are_one_to_a_hundred_allowed_characters() {
        assert!(valid_host_id("host-1.pool_a:9"));
        assert!(valid_host_id(&"a".repeat(100)));
        assert!(!valid_host_id(""));
        assert!(!valid_host_id(&"a".repeat(101)));
        assert!(!valid_host_id("bad host!"));
        assert!(!valid_host_id("hôst"));
    }

    #[test]
    fn a_failed_entrys_error_is_cut_at_a_thousand_utf16_units() {
        assert_eq!(truncate_utf16(&"x".repeat(1500), 1000).len(), 1000);
        assert_eq!(truncate_utf16("short", 1000), "short");
        // A character outside the BMP is two UTF-16 units; it is never split.
        let s = format!("{}{}", "x".repeat(999), '\u{1F600}');
        assert_eq!(truncate_utf16(&s, 1000), "x".repeat(999));
    }

    #[test]
    fn loaded_entries_are_read_strictly_naming_the_index() {
        let entry = |address: &str, version: Option<i64>, state: Option<&str>| LoadedEntry {
            address: Some(address.into()),
            version,
            state: state.map(str::to_string),
            error: None,
        };
        let code_and_message = |r: Result<LoadedVersion, UseCaseError>| {
            let e = r.unwrap_err();
            (e.code().to_string(), e.message().to_string())
        };
        let (code, message) = code_and_message(parse_loaded_entry(
            Some(&entry("not.valid", Some(1), Some("LOADED"))),
            0,
        ));
        assert_eq!(code, "LOADED_INVALID");
        assert!(message.starts_with("loaded[0]: address: "), "{message}");
        let (_, message) = code_and_message(parse_loaded_entry(
            Some(&entry("a.b.c", Some(0), Some("LOADED"))),
            2,
        ));
        assert_eq!(message, "loaded[2]: version must be a positive integer");
        let (_, message) = code_and_message(parse_loaded_entry(
            Some(&entry("a.b.c", Some(1), Some("WEIRD"))),
            1,
        ));
        assert_eq!(
            message,
            "loaded[1]: state must be REGISTERED, LOADED, or FAILED"
        );
        let (_, message) =
            code_and_message(parse_loaded_entry(Some(&entry("a.b.c", Some(1), None)), 1));
        assert_eq!(message, "loaded[1]: state is required");
        let (_, message) = code_and_message(parse_loaded_entry(None, 3));
        assert_eq!(message, "loaded[3]: entry is required");
        let failed = parse_loaded_entry(Some(&entry("a.b.c", Some(1), Some("FAILED"))), 0).unwrap();
        assert_eq!(failed.state, LoadState::Failed(String::new()));
    }
}
