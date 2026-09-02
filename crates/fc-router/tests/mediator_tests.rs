//! HttpMediator Unit Tests
//!
//! Tests for:
//! - Successful message delivery
//! - HTTP status code handling
//! - Circuit breaker behavior
//! - Retry logic
//! - Custom delay parsing from response
//! - Auth token handling

use std::time::Duration;
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use fc_common::{MediationResult, MediationType, Message};
use fc_router::{HttpMediator, HttpMediatorConfig, Mediator};

fn create_test_message(target: &str) -> Message {
    Message {
        id: "msg-1".to_string(),
        pool_code: "DEFAULT".to_string(),
        auth_token: None,
        signing_secret: None,
        mediation_type: MediationType::HTTP,
        mediation_target: target.to_string(),
        message_group_id: None,
        high_priority: false,
        dispatch_mode: fc_common::DispatchMode::default(),
        dispatch_mode_specified: true,
    }
}

fn create_test_message_with_auth(target: &str, token: &str) -> Message {
    Message {
        id: "msg-auth".to_string(),
        pool_code: "DEFAULT".to_string(),
        auth_token: Some(token.to_string()),
        signing_secret: None,
        mediation_type: MediationType::HTTP,
        mediation_target: target.to_string(),
        message_group_id: None,
        high_priority: false,
        dispatch_mode: fc_common::DispatchMode::default(),
        dispatch_mode_specified: true,
    }
}

#[tokio::test]
async fn test_successful_delivery() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webhook"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ack": true})))
        .expect(1)
        .mount(&mock_server)
        .await;

    let mediator = HttpMediator::new();
    let message = create_test_message(&format!("{}/webhook", mock_server.uri()));

    let outcome = mediator.mediate(&message).await;

    assert_eq!(outcome.result, MediationResult::Success);
    assert!(outcome.error_message.is_none());
}

#[tokio::test]
async fn test_successful_delivery_empty_response() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webhook"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&mock_server)
        .await;

    let mediator = HttpMediator::new();
    let message = create_test_message(&format!("{}/webhook", mock_server.uri()));

    let outcome = mediator.mediate(&message).await;

    assert_eq!(outcome.result, MediationResult::Success);
}

#[tokio::test]
async fn test_auth_token_sent() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/secure-webhook"))
        .and(header("Authorization", "Bearer test-token-123"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&mock_server)
        .await;

    let mediator = HttpMediator::new();
    let message = create_test_message_with_auth(
        &format!("{}/secure-webhook", mock_server.uri()),
        "test-token-123",
    );

    let outcome = mediator.mediate(&message).await;

    assert_eq!(outcome.result, MediationResult::Success);
}

#[tokio::test]
async fn test_ack_false_with_custom_delay() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webhook"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"ack": false, "delaySeconds": 60})),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = HttpMediatorConfig {
        max_retries: 1, // Don't retry for this test
        ..Default::default()
    };
    let mediator = HttpMediator::with_config(config);
    let message = create_test_message(&format!("{}/webhook", mock_server.uri()));

    let outcome = mediator.mediate(&message).await;

    assert_eq!(outcome.result, MediationResult::ErrorProcess);
    assert_eq!(outcome.delay_seconds, Some(60));
}

/// `delaySeconds` on an `ack: false` body is a *floor* on the pool's own
/// backoff curve, not a fixed delay (`docs/wire-contract.md`) — when
/// absent, there is no floor, so this must be 0, not the 429 path's 30s
/// fallback. Found while building the mediation conformance runner
/// (corpus case `deferred-ack-false` pins exactly this).
#[tokio::test]
async fn test_ack_false_without_delay_seconds_has_no_floor() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webhook"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ack": false})))
        .expect(1)
        .mount(&mock_server)
        .await;

    let config = HttpMediatorConfig {
        max_retries: 1,
        ..Default::default()
    };
    let mediator = HttpMediator::with_config(config);
    let message = create_test_message(&format!("{}/webhook", mock_server.uri()));

    let outcome = mediator.mediate(&message).await;

    assert_eq!(outcome.result, MediationResult::ErrorProcess);
    assert_eq!(outcome.delay_seconds, Some(0));
}

/// Ledger A-04: the outcome must carry the target's real status, not a
/// hardcoded 200 — 201/202/204 mean different things to an operator
/// reading a trace.
#[tokio::test]
async fn test_success_carries_real_2xx_status() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webhook"))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&mock_server)
        .await;

    let mediator = HttpMediator::new();
    let message = create_test_message(&format!("{}/webhook", mock_server.uri()));

    let outcome = mediator.mediate(&message).await;

    assert_eq!(outcome.result, MediationResult::Success);
    assert_eq!(outcome.status_code, Some(201));
}

/// Ledger A-05: `{"ack":true,"flushGroup":true}` sets `flush_group` on
/// the outcome. No `delaySeconds` in the body -> `None`.
#[tokio::test]
async fn test_success_flush_group() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webhook"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ack": true,
                "flushGroup": true,
            })),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let mediator = HttpMediator::new();
    let message = create_test_message(&format!("{}/webhook", mock_server.uri()));

    let outcome = mediator.mediate(&message).await;

    assert_eq!(outcome.result, MediationResult::Success);
    assert_eq!(outcome.status_code, Some(200));
    assert!(outcome.flush_group);
    assert_eq!(outcome.delay_seconds, None);
}

/// `delaySeconds` alongside `flushGroup: true` sets the suppression
/// window (`docs/wire-contract.md`), carried through on the *success*
/// outcome — distinct from the `ack: false` delay path.
#[tokio::test]
async fn test_success_flush_group_with_delay() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webhook"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ack": true,
                "flushGroup": true,
                "delaySeconds": 20,
            })),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let mediator = HttpMediator::new();
    let message = create_test_message(&format!("{}/webhook", mock_server.uri()));

    let outcome = mediator.mediate(&message).await;

    assert_eq!(outcome.result, MediationResult::Success);
    assert!(outcome.flush_group);
    assert_eq!(outcome.delay_seconds, Some(20));
}

/// A plain 2xx with no `flushGroup` field defaults to `false` — the
/// common case must not accidentally suppress a group.
#[tokio::test]
async fn test_success_without_flush_group_defaults_false() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webhook"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ack": true})))
        .expect(1)
        .mount(&mock_server)
        .await;

    let mediator = HttpMediator::new();
    let message = create_test_message(&format!("{}/webhook", mock_server.uri()));

    let outcome = mediator.mediate(&message).await;

    assert!(!outcome.flush_group);
}

/// Ledger R-05 / A-06: a 3xx is permanent, not retried — 301/302/307 all
/// classify as `ErrorConfig` in a single attempt, carrying their own
/// status code, despite a retry budget that would otherwise allow more
/// attempts.
#[tokio::test]
async fn test_3xx_is_permanent_not_retried() {
    for status in [301u16, 302, 307] {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/webhook"))
            .respond_with(
                ResponseTemplate::new(status).insert_header("Location", "https://example.com/elsewhere"),
            )
            .expect(1) // exactly one attempt — a retry would fail this expectation
            .mount(&mock_server)
            .await;

        let config = HttpMediatorConfig {
            max_retries: 3, // retries are available; the 3xx branch must not use them
            retry_delays: vec![Duration::from_millis(10); 3],
            ..Default::default()
        };
        let mediator = HttpMediator::with_config(config);
        let message = create_test_message(&format!("{}/webhook", mock_server.uri()));

        let outcome = mediator.mediate(&message).await;

        assert_eq!(
            outcome.result,
            MediationResult::ErrorConfig,
            "status {status} should be a permanent ErrorConfig"
        );
        assert_eq!(outcome.status_code, Some(status), "status {status} mismatch");
    }
}

/// Ledger R-05: the client must never actually follow the redirect. If it
/// did, the POST body would be replayed (307) or dropped (301/302) to a
/// second server nobody configured as the mediation target — either way,
/// that second server must never see a request at all.
#[tokio::test]
async fn test_redirect_body_never_replayed_to_target() {
    // No `Mock` mounted on this server at all — request recording is on
    // by default regardless, so any request that reached it would still
    // show up in `received_requests()`.
    let redirect_target_server = MockServer::start().await;

    let origin_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/webhook"))
        .respond_with(ResponseTemplate::new(307).insert_header(
            "Location",
            format!("{}/should-never-be-called", redirect_target_server.uri()).as_str(),
        ))
        .expect(1)
        .mount(&origin_server)
        .await;

    let mediator = HttpMediator::new();
    let message = create_test_message(&format!("{}/webhook", origin_server.uri()));

    let outcome = mediator.mediate(&message).await;

    assert_eq!(outcome.result, MediationResult::ErrorConfig);
    assert_eq!(outcome.status_code, Some(307));

    let received = redirect_target_server.received_requests().await;
    assert_eq!(
        received.map(|r| r.len()),
        Some(0),
        "the redirect target must never see a request — the client must not follow it"
    );
}

#[tokio::test]
async fn test_400_bad_request() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webhook"))
        .respond_with(ResponseTemplate::new(400))
        .expect(1)
        .mount(&mock_server)
        .await;

    let mediator = HttpMediator::new();
    let message = create_test_message(&format!("{}/webhook", mock_server.uri()));

    let outcome = mediator.mediate(&message).await;

    assert_eq!(outcome.result, MediationResult::ErrorConfig);
    assert_eq!(outcome.status_code, Some(400));
}

#[tokio::test]
async fn test_401_unauthorized() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webhook"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&mock_server)
        .await;

    let mediator = HttpMediator::new();
    let message = create_test_message(&format!("{}/webhook", mock_server.uri()));

    let outcome = mediator.mediate(&message).await;

    assert_eq!(outcome.result, MediationResult::ErrorConfig);
    assert_eq!(outcome.status_code, Some(401));
}

#[tokio::test]
async fn test_403_forbidden() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webhook"))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&mock_server)
        .await;

    let mediator = HttpMediator::new();
    let message = create_test_message(&format!("{}/webhook", mock_server.uri()));

    let outcome = mediator.mediate(&message).await;

    assert_eq!(outcome.result, MediationResult::ErrorConfig);
    assert_eq!(outcome.status_code, Some(403));
}

#[tokio::test]
async fn test_404_not_found() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webhook"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&mock_server)
        .await;

    let mediator = HttpMediator::new();
    let message = create_test_message(&format!("{}/webhook", mock_server.uri()));

    let outcome = mediator.mediate(&message).await;

    assert_eq!(outcome.result, MediationResult::ErrorConfig);
    assert_eq!(outcome.status_code, Some(404));
}

/// Ledger R-57: sweeps the 5xx boundary plus a transport failure, table-
/// driven. `502`/`503`/`504` mean "target unavailable" — never reached a
/// working app — so they stay retryable (`ErrorProcess`, 30s delay); a
/// transport failure (nothing answered at all) lands in the same family
/// (`ErrorConnection`, 30s). Every OTHER 5xx (500, 501, 505, …) means "the
/// app was reached and answered, with a fault" — retrying the identical
/// request cannot help, so it is now a permanent `ErrorConfig`, exactly
/// like a 4xx, with no delay. `499` is included as the boundary just below
/// 500 — an ordinary 4xx, unaffected by this ruling — so the table reads
/// as a sweep across the boundary rather than an arbitrary status list.
///
/// This replaces `test_500_server_error_with_retry` /
/// `test_500_exhausts_retries` / `test_501_not_implemented` /
/// `test_502_bad_gateway` / `test_503_service_unavailable`, which pinned
/// the pre-ruling behaviour (500 retried forever; 502/503 tested
/// separately). See the conformance runner
/// (`tests/mediation_conformance_test.rs`) for the accompanying
/// warning/breaker assertions this test doesn't cover.
#[tokio::test]
async fn test_5xx_boundary_r57() {
    #[derive(Clone, Copy)]
    enum Given {
        Status(u16),
        Transport,
    }

    struct Case {
        given: Given,
        expected_result: MediationResult,
        expected_status: Option<u16>,
        expected_delay: Option<u32>,
    }

    let cases = vec![
        Case {
            given: Given::Status(499),
            expected_result: MediationResult::ErrorConfig,
            expected_status: Some(499),
            expected_delay: None,
        },
        Case {
            given: Given::Status(500),
            expected_result: MediationResult::ErrorConfig,
            expected_status: Some(500),
            expected_delay: None,
        },
        Case {
            given: Given::Status(501),
            expected_result: MediationResult::ErrorConfig,
            expected_status: Some(501),
            expected_delay: None,
        },
        Case {
            given: Given::Status(502),
            expected_result: MediationResult::ErrorProcess,
            expected_status: Some(502),
            expected_delay: Some(30),
        },
        Case {
            given: Given::Status(503),
            expected_result: MediationResult::ErrorProcess,
            expected_status: Some(503),
            expected_delay: Some(30),
        },
        Case {
            given: Given::Status(504),
            expected_result: MediationResult::ErrorProcess,
            expected_status: Some(504),
            expected_delay: Some(30),
        },
        Case {
            given: Given::Status(505),
            expected_result: MediationResult::ErrorConfig,
            expected_status: Some(505),
            expected_delay: None,
        },
        Case {
            given: Given::Transport,
            expected_result: MediationResult::ErrorConnection,
            expected_status: None,
            expected_delay: Some(30),
        },
    ];

    for case in cases {
        // One attempt per case: this test pins classification, not the
        // retry loop (that's `retry.rs`'s own unit tests).
        let config = HttpMediatorConfig {
            max_retries: 1,
            ..Default::default()
        };
        let mediator = HttpMediator::with_config(config);

        // Keep the mock server alive for the duration of the request by
        // holding it in scope through the `match`.
        let (target, _guard) = match case.given {
            Given::Status(status) => {
                let mock_server = MockServer::start().await;
                Mock::given(method("POST"))
                    .and(path("/webhook"))
                    .respond_with(ResponseTemplate::new(status))
                    .expect(1)
                    .mount(&mock_server)
                    .await;
                let target = format!("{}/webhook", mock_server.uri());
                (target, Some(mock_server))
            }
            Given::Transport => ("http://127.0.0.1:59999/webhook".to_string(), None),
        };

        let message = create_test_message(&target);
        let outcome = mediator.mediate(&message).await;

        let label = match case.given {
            Given::Status(s) => s.to_string(),
            Given::Transport => "transport error".to_string(),
        };
        assert_eq!(
            outcome.result, case.expected_result,
            "{label}: wrong MediationResult"
        );
        assert_eq!(
            outcome.status_code, case.expected_status,
            "{label}: wrong status_code"
        );
        assert_eq!(
            outcome.delay_seconds, case.expected_delay,
            "{label}: wrong delay_seconds"
        );
    }
}

#[tokio::test]
async fn test_connection_error() {
    let mediator = HttpMediator::new();
    // Use a port that's definitely not listening
    let message = create_test_message("http://127.0.0.1:59999/webhook");

    let outcome = mediator.mediate(&message).await;

    assert_eq!(outcome.result, MediationResult::ErrorConnection);
    assert!(outcome.error_message.is_some());
}

// Circuit breaker tests live in `src/circuit_breaker_registry.rs` now that
// breaker state is owned by `CircuitBreakerRegistry` (per-endpoint, shared
// across pools) rather than by `HttpMediator`.

#[tokio::test]
async fn test_timeout_handling() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webhook"))
        .respond_with(
            ResponseTemplate::new(200).set_delay(Duration::from_secs(10)), // Long delay
        )
        .mount(&mock_server)
        .await;

    let config = HttpMediatorConfig {
        timeout: Duration::from_millis(100), // Short timeout
        max_retries: 1,
        ..Default::default()
    };
    let mediator = HttpMediator::with_config(config);
    let message = create_test_message(&format!("{}/webhook", mock_server.uri()));

    let outcome = mediator.mediate(&message).await;

    assert_eq!(outcome.result, MediationResult::ErrorConnection);
    assert!(outcome.error_message.as_ref().unwrap().contains("timeout"));
}

#[tokio::test]
async fn test_payload_sent_correctly() {
    let mock_server = MockServer::start().await;

    // The mediator sends {"messageId":"<id>"} matching Java behavior, NOT the message payload
    let expected_payload = serde_json::json!({"messageId": "msg-1"});

    Mock::given(method("POST"))
        .and(path("/webhook"))
        .and(body_json(&expected_payload))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&mock_server)
        .await;

    let mediator = HttpMediator::new();
    let message = create_test_message(&format!("{}/webhook", mock_server.uri()));

    let outcome = mediator.mediate(&message).await;

    assert_eq!(outcome.result, MediationResult::Success);
}

#[tokio::test]
async fn test_422_unprocessable_entity() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webhook"))
        .respond_with(ResponseTemplate::new(422))
        .expect(1)
        .mount(&mock_server)
        .await;

    let mediator = HttpMediator::new();
    let message = create_test_message(&format!("{}/webhook", mock_server.uri()));

    let outcome = mediator.mediate(&message).await;

    // 422 is a client error, should be ErrorConfig
    assert_eq!(outcome.result, MediationResult::ErrorConfig);
    assert_eq!(outcome.status_code, Some(422));
}

// 502/503/504 are covered by `test_5xx_boundary_r57` above, table-driven
// alongside 499/500/501/505/transport-error.

#[tokio::test]
async fn test_mediator_default_config() {
    // Smoke test: default config builds and mediates without panic.
    let mediator = HttpMediator::new();
    let message = create_test_message("http://127.0.0.1:59999/webhook");
    let _ = mediator.mediate(&message).await;
}
