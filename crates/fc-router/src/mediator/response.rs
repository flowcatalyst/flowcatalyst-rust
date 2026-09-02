//! Map an HTTP response from a mediation target to a `MediationOutcome`.
//!
//! Status-code dispatch:
//! - **2xx with `ack: true`** (or no body) → `Success`, carrying the
//!   target's real status code (ledger A-04) and, if the body carried
//!   `flushGroup: true`, `flush_group = true` plus that body's
//!   `delaySeconds` as the suppression window (ledger A-05).
//! - **2xx with `ack: false`** → `Deferred` (ledger 22b) with the target's
//!   `delaySeconds` (a floor on the pool's own backoff curve, defaulting
//!   to 0 — no floor — when absent) — the target is healthy but
//!   asking us to retry later. Breaker-neutral, like `RateLimited`: the
//!   endpoint answered and declined the work, which is not evidence it is
//!   unhealthy.
//! - **3xx** → `ErrorConfig` (ledger R-05 / A-06). The client never
//!   follows a redirect (see `inner::make_client_builder`), so a 3xx is
//!   the target's own final answer: permanent, not retryable, warned
//!   like a 4xx, naming the `Location` header.
//! - **400 / 401 / 403 / 404 / 501** → `ErrorConfig`. These don't retry
//!   and emit a configuration warning.
//! - **429** → `RateLimited` with the `Retry-After` header (default 30).
//!   The pool nacks with that delay and does NOT consume the retry
//!   budget or trip the circuit breaker.
//! - **Other 4xx** → `ErrorConfig`, warned like a named 4xx.
//! - **502 / 503 / 504** → `ErrorProcess` — retryable transient: the
//!   target was unreachable/unavailable, not wrong.
//! - **Every other 5xx** (500, 505, …) → `ErrorConfig` (ledger R-57): the
//!   app was reached and answered — with a fault, but it ran — so
//!   retrying the identical request cannot help. Same warning treatment
//!   as a 4xx.
//! - **Anything else** → `ErrorProcess`.

use std::sync::Arc;

use fc_common::{MediationOutcome, MediationResult, Message, WarningCategory, WarningSeverity};
use reqwest::Response;
use serde::Deserialize;
use tracing::{debug, warn};

use crate::warning::WarningService;

#[derive(Debug, Deserialize, Default)]
struct MediationResponse {
    #[serde(default = "default_ack")]
    ack: bool,
    #[serde(rename = "delaySeconds")]
    delay_seconds: Option<u32>,
    /// Ledger A-05: any target may ask the router to stop delivering the
    /// rest of this message's group instead of continuing message-by-
    /// message. Only meaningful alongside `ack: true` — the wire field is
    /// parsed here; the pool-side suppression registry is a later lane.
    #[serde(rename = "flushGroup", default)]
    flush_group: bool,
}

fn default_ack() -> bool {
    true
}

pub(super) async fn classify(
    response: Response,
    message: &Message,
    warning_service: &Arc<WarningService>,
) -> MediationOutcome {
    let status = response.status();
    let status_code = status.as_u16();

    if status.is_success() {
        // Parse response body for ack, delaySeconds and flushGroup.
        if let Ok(body) = response.text().await {
            if let Ok(resp) = serde_json::from_str::<MediationResponse>(&body) {
                if !resp.ack {
                    // `delaySeconds` here is a *floor* on the pool's own
                    // deferred backoff curve (docs/wire-contract.md), not a
                    // fixed delay: absent, there is no floor and the curve
                    // alone governs, so this defaults to 0 — not the 429
                    // path's 30s default just below, which is a real
                    // fallback delay in the absence of `Retry-After`.
                    let delay = resp.delay_seconds.unwrap_or(0);
                    debug!(
                        message_id = %message.id,
                        delay_seconds = delay,
                        "Target returned ack=false with delay"
                    );
                    // Ledger 22b: this is a deferral, not a failure — the
                    // target is healthy and just declined the work right
                    // now. Breaker-neutral (see `MediationResult::Deferred`'s
                    // doc comment), retried in place with the target's
                    // requested delay.
                    return MediationOutcome::deferred(status_code, Some(delay));
                }

                if resp.flush_group {
                    debug!(
                        message_id = %message.id,
                        status_code = status_code,
                        "Message delivered; target requested flushGroup"
                    );
                    let mut outcome = MediationOutcome::success(status_code);
                    outcome.flush_group = true;
                    // delaySeconds, when present alongside flushGroup, sets
                    // the suppression window (docs/wire-contract.md).
                    outcome.delay_seconds = resp.delay_seconds;
                    return outcome;
                }
            }
        }

        debug!(
            message_id = %message.id,
            status_code = status_code,
            "Message delivered successfully"
        );
        return MediationOutcome::success(status_code);
    }

    if status.is_redirection() {
        // Ledger R-05 / A-06: unfollowed 3xx is permanent, not retryable —
        // the target will answer identically forever, and following it
        // instead would silently drop the POST body (301/302/303 downgrade
        // to a bodyless GET per RFC 7231) while recording a false success.
        let location = response
            .headers()
            .get("Location")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("<no Location header>")
            .to_string();
        warn!(
            message_id = %message.id,
            status_code = status_code,
            location = %location,
            "Redirect not followed - configuration error"
        );
        emit_config_warning(
            warning_service,
            &message.id,
            &message.mediation_target,
            status_code,
            &format!("Redirect not followed (Location: {})", location),
        );
        return MediationOutcome::error_config(
            status_code,
            format!(
                "HTTP {}: Redirect not followed (Location: {})",
                status_code, location
            ),
        );
    }

    if status_code == 400 {
        warn!(
            message_id = %message.id,
            status_code = status_code,
            "Bad request - configuration error"
        );
        emit_config_warning(
            warning_service,
            &message.id,
            &message.mediation_target,
            status_code,
            "Bad Request",
        );
        return MediationOutcome::error_config(status_code, "HTTP 400: Bad request".to_string());
    }

    if status_code == 401 || status_code == 403 {
        let desc = if status_code == 401 {
            "Unauthorized"
        } else {
            "Forbidden"
        };
        warn!(
            message_id = %message.id,
            status_code = status_code,
            "Authentication/authorization error"
        );
        emit_config_warning(
            warning_service,
            &message.id,
            &message.mediation_target,
            status_code,
            desc,
        );
        return MediationOutcome::error_config(
            status_code,
            format!("HTTP {}: Auth error", status_code),
        );
    }

    if status_code == 404 {
        warn!(
            message_id = %message.id,
            status_code = status_code,
            "Endpoint not found"
        );
        emit_config_warning(
            warning_service,
            &message.id,
            &message.mediation_target,
            status_code,
            "Not Found",
        );
        return MediationOutcome::error_config(status_code, "HTTP 404: Not found".to_string());
    }

    if status_code == 429 {
        // Healthy destination throttling us. Return RateLimited so the
        // pool applies Retry-After without consuming the retry budget or
        // tripping the circuit breaker.
        let retry_after = response
            .headers()
            .get("Retry-After")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(30);
        warn!(
            message_id = %message.id,
            status_code = status_code,
            retry_after = retry_after,
            "Rate limited (429) - will retry"
        );
        return MediationOutcome::rate_limited(retry_after);
    }

    if status_code == 501 {
        warn!(
            message_id = %message.id,
            status_code = status_code,
            "Not implemented"
        );
        emit_config_warning(
            warning_service,
            &message.id,
            &message.mediation_target,
            status_code,
            "Not Implemented",
        );
        return MediationOutcome::error_config(
            status_code,
            "HTTP 501: Not implemented".to_string(),
        );
    }

    if status.is_client_error() {
        // Generic 4xx fallback (no named branch above claimed this status).
        // Same permanence and the same deletion as a 400/404, so the same
        // notice: an operator told about a 404 and not a 422 is a gap, not
        // a decision (conformance corpus `config-error-other-4xx`).
        warn!(
            message_id = %message.id,
            status_code = status_code,
            "Client error"
        );
        emit_config_warning(
            warning_service,
            &message.id,
            &message.mediation_target,
            status_code,
            "Client error",
        );
        return MediationOutcome::error_config(
            status_code,
            format!("HTTP {}: Client error", status_code),
        );
    }

    if status.is_server_error() {
        if status_code == 502 || status_code == 503 || status_code == 504 {
            // "Target unavailable": never reached a working app — a dead
            // gateway, an overloaded backend, an upstream timeout. Nothing
            // about the message is wrong, so hold at the broker with
            // backoff rather than dropping it.
            warn!(
                message_id = %message.id,
                status_code = status_code,
                "Server error - target unavailable, will retry"
            );
            return MediationOutcome {
                result: MediationResult::ErrorProcess,
                delay_seconds: Some(30),
                status_code: Some(status_code),
                error_message: Some(format!("HTTP {}: Server error", status_code)),
                flush_group: false,
                pre_flight: false,
            };
        }

        // Ledger R-57: every other 5xx (500, 505, 506, …) — the app was
        // reached and answered, with a fault, but it ran. Retrying the
        // identical request cannot help, so this is permanent like a 4xx,
        // with the same warning treatment: the warning is the deleted
        // message's only trace.
        warn!(
            message_id = %message.id,
            status_code = status_code,
            "Server error - permanent, configuration error"
        );
        emit_config_warning(
            warning_service,
            &message.id,
            &message.mediation_target,
            status_code,
            "Server error",
        );
        return MediationOutcome::error_config(
            status_code,
            format!("HTTP {}: Server error", status_code),
        );
    }

    warn!(
        message_id = %message.id,
        status_code = status_code,
        "Unexpected status code"
    );
    MediationOutcome::error_process(Some(30), format!("HTTP {}: Unexpected status", status_code))
}

/// Push a configuration warning to the `WarningService`. 501 is upgraded
/// to `Critical`; everything else is `Error`.
fn emit_config_warning(
    warning_service: &Arc<WarningService>,
    message_id: &str,
    target: &str,
    status_code: u16,
    description: &str,
) {
    let severity = if status_code == 501 {
        WarningSeverity::Critical
    } else {
        WarningSeverity::Error
    };
    warning_service.add_warning(
        WarningCategory::Configuration,
        severity,
        format!(
            "HTTP {} {} for message {}: Target: {}",
            status_code, description, message_id, target
        ),
        "HttpMediator".to_string(),
    );
}
