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
//!   The pool retries it in place with that delay as the backoff floor;
//!   it does not trip the circuit breaker.
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

    if status_code == 429 {
        // Healthy destination throttling us. Return RateLimited so the
        // pool retries in place with Retry-After as the floor, without
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

    if let Some(fault) = config_fault(status_code) {
        warn!(message_id = %message.id, status_code = status_code, "{}", fault.log);
        emit_config_warning(
            warning_service,
            &message.id,
            &message.mediation_target,
            status_code,
            fault.warning,
        );
        return MediationOutcome::error_config(
            status_code,
            format!("HTTP {}: {}", status_code, fault.detail),
        );
    }

    if (502..=504).contains(&status_code) {
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

    warn!(
        message_id = %message.id,
        status_code = status_code,
        "Unexpected status code"
    );
    MediationOutcome::error_process(Some(30), format!("HTTP {}: Unexpected status", status_code))
}

/// How a status the target answered with, and no retry can fix, is logged,
/// warned about and described in the outcome.
struct ConfigFault {
    /// The log line for the delivery.
    log: &'static str,
    /// The description in the operator warning.
    warning: &'static str,
    /// The tail of the outcome's `HTTP {code}: {detail}` message.
    detail: &'static str,
}

/// The permanent-failure statuses: 400, 401, 403, 404, 501, any other 4xx
/// (conformance corpus `config-error-other-4xx`: an operator told about a
/// 404 and not a 422 is a gap, not a decision) and every other 5xx
/// (ledger R-57: the app was reached and answered with a fault, but it ran,
/// so retrying the identical request cannot help; the warning is the
/// deleted message's only trace). 429 and 502/503/504 are handled by the
/// caller, since they are retried.
fn config_fault(status_code: u16) -> Option<ConfigFault> {
    let (log, warning, detail) = match status_code {
        400 => (
            "Bad request - configuration error",
            "Bad Request",
            "Bad request",
        ),
        401 => (
            "Authentication/authorization error",
            "Unauthorized",
            "Auth error",
        ),
        403 => (
            "Authentication/authorization error",
            "Forbidden",
            "Auth error",
        ),
        404 => ("Endpoint not found", "Not Found", "Not found"),
        501 => ("Not implemented", "Not Implemented", "Not implemented"),
        429 | 502..=504 => return None,
        code if (400..500).contains(&code) => ("Client error", "Client error", "Client error"),
        code if (500..600).contains(&code) => (
            "Server error - permanent, configuration error",
            "Server error",
            "Server error",
        ),
        _ => return None,
    };
    Some(ConfigFault {
        log,
        warning,
        detail,
    })
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
