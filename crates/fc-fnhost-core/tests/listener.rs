//! The private listener (Java `FnHttpServerTest` H1-H14,
//! `FnHttpServerObserverWiringTest`), over real HTTP against a scripted
//! runtime behind the `Invoker` seam.

mod support;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use fc_fnhost_core::clock::Clock;
use fc_fnhost_core::host::Listener;
use serde_json::json;
use support::listener::{
    doc, entry, foreign_key, mint, raw, signed, timestamp, with, Claims, Harness, Options, TestJwks,
};

const ADDR: &str = "app.orders.ship";
const ADDR_B: &str = "app.orders.bill";

fn fpath(address: &str, rest: &str) -> String {
    format!("/functions/{address}{rest}")
}

// ── H1: request shape and routing ────────────────────────────────────────

#[tokio::test]
async fn h1_request_shape_and_routing_rules() {
    let h = Harness::start(doc(vec![entry(
        ADDR,
        1,
        "live",
        "echo",
        10,
        json!([{"path": "/echo/{id}", "auth": "none"}, {"path": "/onlypost", "auth": "none", "methods": ["POST"]}]),
    )]))
    .await;
    let label = format!("{ADDR}@1");

    let ok = h
        .get(
            &fpath(ADDR, "/echo/a%20b?y=hello+world&y=again"),
            &[("X-Test-Custom", "hi"), ("X-Correlation-Id", "corr-1")],
        )
        .await;
    assert_eq!(ok.status, 200, "{}", ok.text());
    let body = ok.json();
    assert_eq!(
        body["path"], "/echo/a%20b",
        "the function path is raw, prefix stripped"
    );
    assert_eq!(body["originalPath"], fpath(ADDR, "/echo/a%20b"));
    assert!(body["originalHost"]
        .as_str()
        .unwrap()
        .starts_with("127.0.0.1:"));
    assert_eq!(
        body["pathParams"]["id"], "a b",
        "params are percent-decoded"
    );
    assert_eq!(body["query"]["y"], json!(["hello world", "again"]));
    assert_eq!(body["headers"]["x-test-custom"], json!(["hi"]));
    assert_eq!(body["caller"]["kind"], "Anonymous");
    assert_eq!(body["remoteAddress"], "127.0.0.1");
    assert_eq!(body["version"], 1);
    assert_eq!(body["method"], "GET");
    assert_eq!(body["correlationId"], "corr-1");
    assert_eq!(body["causationId"], json!(null));
    let invocation_id = body["invocationId"].as_str().unwrap();
    assert_eq!(invocation_id.len(), 13, "a raw TSID");
    assert!(
        body["remainingMs"].as_u64().unwrap() > 20_000,
        "the default 30 s deadline"
    );

    let missing = h.get(&fpath(ADDR, "/does-not-exist"), &[]).await;
    assert_eq!(missing.status, 404);
    assert_eq!(
        missing.text(),
        r#"{"error":"ENDPOINT_NOT_FOUND","message":"no endpoint matches this path"}"#
    );
    assert_eq!(
        missing.header("content-type").as_deref(),
        Some("application/json")
    );

    let wrong_method = h.get(&fpath(ADDR, "/onlypost"), &[]).await;
    assert_eq!(wrong_method.status, 405);
    assert_eq!(wrong_method.header("allow").as_deref(), Some("POST"));
    assert_eq!(wrong_method.error(), "METHOD_NOT_ALLOWED");

    let zero = h.get(&fpath(ADDR, ":0/echo/1"), &[]).await;
    assert_eq!(zero.status, 400);
    assert_eq!(
        zero.text(),
        r#"{"error":"VERSION_INVALID","message":"version must be a positive integer"}"#
    );
    let bad_address = h.get("/functions/Not.An.Address/x", &[]).await;
    assert_eq!(bad_address.status, 400);
    assert_eq!(
        bad_address.text(),
        r#"{"error":"ADDRESS_INVALID","message":"invalid function address"}"#
    );
    let unknown = h.get(&fpath("app.orders.nope", "/echo/1"), &[]).await;
    assert_eq!(unknown.status, 404);
    assert_eq!(
        unknown.text(),
        r#"{"error":"FUNCTION_NOT_FOUND","message":"no such function"}"#
    );
    let elsewhere = h.get("/health", &[]).await;
    assert_eq!(elsewhere.status, 404);
    assert_eq!(
        elsewhere.text(),
        r#"{"error":"NOT_FOUND","message":"not found"}"#
    );

    assert_eq!(
        h.probes().invocations(&label),
        1,
        "only the first call reached the function"
    );
}

// ── H2: webhook signatures ───────────────────────────────────────────────

fn webhook_entry(address: &str, secret: Option<&str>) -> serde_json::Value {
    let e = entry(
        address,
        1,
        "live",
        "echo",
        10,
        json!([{"path": "/events/*", "auth": "webhook"}]),
    );
    match secret {
        Some(secret) => with(
            e,
            json!({"webhookSigningSecret": secret, "clientId": "clt_1"}),
        ),
        None => e,
    }
}

#[tokio::test]
async fn h2_webhook_signature_verification() {
    let h = Harness::start(doc(vec![webhook_entry(ADDR, Some("wh-secret-1"))])).await;
    let label = format!("{ADDR}@1");
    let now = h.clock.now();
    let body =
        br#"{"id":"evt-9","type":"a:b:c:d","attemptNumber":1,"correlationId":"flow-7","data":{}}"#;
    let ts = timestamp(now);
    let sig = signed("wh-secret-1", &ts, body);
    let path = fpath(ADDR, "/events/x");
    let post = |body: &'static [u8], headers: Vec<(&'static str, String)>| {
        let h = &h;
        let path = path.clone();
        async move {
            let headers: Vec<(&str, &str)> =
                headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
            h.post(&path, body, &headers).await
        }
    };

    let ok = post(
        body,
        vec![
            ("X-FlowCatalyst-Signature", sig.clone()),
            ("X-FlowCatalyst-Timestamp", ts.clone()),
            ("X-Correlation-Id", "hdr-1".into()),
        ],
    )
    .await;
    assert_eq!(ok.status, 200, "{}", ok.text());
    assert_eq!(ok.json()["caller"]["kind"], "Platform");
    assert_eq!(
        ok.json()["correlationId"],
        "flow-7",
        "the inbound event's correlation wins"
    );
    assert_eq!(ok.json()["causationId"], "evt-9");
    assert_eq!(h.probes().invocations(&label), 1);

    let stale_ts = timestamp(now - chrono::Duration::seconds(301));
    let future_ts = timestamp(now + chrono::Duration::seconds(61));
    let other_ts = timestamp(now + chrono::Duration::seconds(5));
    type Case<'a> = (&'a [u8], Vec<(&'a str, String)>, &'a str);
    let cases: Vec<Case> = vec![
        (
            body,
            vec![
                (
                    "X-FlowCatalyst-Signature",
                    signed("wrong-secret", &ts, body),
                ),
                ("X-FlowCatalyst-Timestamp", ts.clone()),
            ],
            "INVALID_SIGNATURE",
        ),
        (
            b"{\"hello\":\"tampered\"}",
            vec![
                ("X-FlowCatalyst-Signature", sig.clone()),
                ("X-FlowCatalyst-Timestamp", ts.clone()),
            ],
            "INVALID_SIGNATURE",
        ),
        (
            body,
            vec![
                ("X-FlowCatalyst-Signature", sig.clone()),
                ("X-FlowCatalyst-Timestamp", other_ts),
            ],
            "INVALID_SIGNATURE",
        ),
        (
            body,
            vec![
                (
                    "X-FlowCatalyst-Signature",
                    signed("wh-secret-1", &stale_ts, body),
                ),
                ("X-FlowCatalyst-Timestamp", stale_ts),
            ],
            "TIMESTAMP_EXPIRED",
        ),
        (
            body,
            vec![
                (
                    "X-FlowCatalyst-Signature",
                    signed("wh-secret-1", &future_ts, body),
                ),
                ("X-FlowCatalyst-Timestamp", future_ts),
            ],
            "TIMESTAMP_IN_FUTURE",
        ),
        (
            body,
            vec![("X-FlowCatalyst-Timestamp", ts.clone())],
            "MISSING_SIGNATURE",
        ),
        (
            body,
            vec![("X-FlowCatalyst-Signature", sig.clone())],
            "MISSING_TIMESTAMP",
        ),
    ];
    for (case_body, headers, reason) in cases {
        let refused = post(case_body, headers).await;
        assert_eq!(refused.status, 401, "{reason}");
        assert_eq!(
            refused.text(),
            format!(r#"{{"error":"UNAUTHORIZED","message":"{reason}"}}"#)
        );
        assert_eq!(refused.header("www-authenticate"), None);
    }
    assert_eq!(
        h.probes().invocations(&label),
        1,
        "no refusal reached the function"
    );

    // webhook ⇒ POST, whatever the manifest stores
    let get = h.get(&path, &[]).await;
    assert_eq!(get.status, 405);
    assert_eq!(get.header("allow").as_deref(), Some("POST"));
}

// ── H3: rotation window, no secret ───────────────────────────────────────

#[tokio::test]
async fn h3_secret_rotation_window() {
    let h = Harness::start(doc(vec![webhook_entry(ADDR, Some("rot-v1"))])).await;
    let body = b"{}";
    let path = fpath(ADDR, "/events/x");
    let call = |secret: &'static str| {
        let h = &h;
        let path = path.clone();
        async move {
            let ts = timestamp(h.clock.now());
            let sig = signed(secret, &ts, body);
            h.post(
                &path,
                body,
                &[
                    ("X-FlowCatalyst-Signature", &sig),
                    ("X-FlowCatalyst-Timestamp", &ts),
                ],
            )
            .await
            .status
        }
    };
    let rotated = doc(vec![webhook_entry(ADDR, Some("rot-v2"))]);
    h.publish(rotated.clone()).await; // reconcile #2: the change
    assert_eq!(
        call("rot-v1").await,
        200,
        "the previous secret is still accepted"
    );
    h.publish(rotated.clone()).await; // reconcile #3: still inside the window
    assert_eq!(call("rot-v1").await, 200);
    h.publish(rotated).await; // reconcile #4: the window has passed
    assert_eq!(call("rot-v1").await, 401, "not for ever");
    assert_eq!(call("rot-v2").await, 200, "the current secret always works");

    let h2 = Harness::start(doc(vec![webhook_entry(ADDR_B, None)])).await;
    let ts = timestamp(h2.clock.now());
    let refused = h2
        .post(
            &fpath(ADDR_B, "/events/x"),
            body,
            &[
                ("X-FlowCatalyst-Signature", &signed("anything", &ts, body)),
                ("X-FlowCatalyst-Timestamp", &ts),
            ],
        )
        .await;
    assert_eq!(refused.status, 401);
    assert_eq!(refused.json()["message"], "NO_SIGNING_SECRET");
    assert_eq!(h2.probes().total_invocations(), 0);
    let beat = h2.control.last_heartbeat();
    assert!(
        support::fakes::states(&beat).contains(&(
            ADDR_B.to_owned(),
            1,
            "FAILED:NO_SIGNING_SECRET".to_owned()
        )),
        "{:?}",
        support::fakes::states(&beat)
    );
}

// ── H4: platform bearer tokens ───────────────────────────────────────────

fn platform_entry(address: &str) -> serde_json::Value {
    entry(
        address,
        1,
        "live",
        "echo",
        10,
        json!([{"path": "/api/*", "auth": "platform"}]),
    )
}

async fn bearer_harness(jwks: &TestJwks) -> Harness {
    Harness::start_with(
        doc(vec![platform_entry(ADDR)]),
        Options {
            platform_url: Some(jwks.url.clone()),
            ..Options::default()
        },
    )
    .await
}

#[tokio::test]
async fn h4_platform_bearer_token() {
    let jwks = TestJwks::start().await;
    let h = bearer_harness(&jwks).await;
    let path = fpath(ADDR, "/api/x");
    let get = |token: String| {
        let h = &h;
        let path = path.clone();
        async move {
            h.get(&path, &[("Authorization", &format!("Bearer {token}"))])
                .await
        }
    };

    let token = jwks.mint(&Claims::new(
        "prn_1",
        "CLIENT",
        "platform:function:function:view",
        &["clt_1"],
    ));
    let ok = get(token).await;
    assert_eq!(ok.status, 200, "{}", ok.text());
    let caller = &ok.json()["caller"];
    assert_eq!(caller["kind"], "Principal");
    assert_eq!(caller["id"], "prn_1");
    assert_eq!(caller["type"], "SERVICE");
    assert_eq!(caller["clientId"], "clt_1");
    assert_eq!(
        caller["permissions"],
        json!(["platform:function:function:view"])
    );
    assert!(
        ok.json()["headers"].get("authorization").is_none(),
        "consumed, so stripped"
    );
    assert_eq!(
        jwks.jwks_requests(),
        1,
        "the first, unknown kid fetches once"
    );

    let expired = get(jwks.mint(&Claims::new("prn_1", "CLIENT", "x", &[]).expires_in(-10))).await;
    assert_eq!(expired.status, 401);
    assert_eq!(
        expired.header("www-authenticate").as_deref(),
        Some("Bearer")
    );
    assert_eq!(
        expired.json()["message"],
        "token has invalid claims: token is expired"
    );

    // the platformUrl is not the issuer: only discovery's issuer is
    let wrong_issuer =
        get(jwks.mint_with_issuer(&jwks.url, &Claims::new("prn_1", "CLIENT", "x", &[]))).await;
    assert_eq!(wrong_issuer.status, 401);
    assert_eq!(
        wrong_issuer.json()["message"],
        "sessiontoken: issuer not accepted"
    );

    let wrong_key = get(mint(
        &foreign_key(),
        "kid-1",
        &jwks.discovery_issuer,
        &Claims::new("prn_1", "CLIENT", "x", &[]),
    ))
    .await;
    assert_eq!(wrong_key.status, 401);
    assert_eq!(wrong_key.json()["message"], "token signature is invalid");

    let garbage = get("not-a-jwt".into()).await;
    assert_eq!(garbage.status, 401);
    assert!(garbage.json()["message"]
        .as_str()
        .unwrap()
        .starts_with("token is malformed: "));

    let no_bearer = h.get(&path, &[("Authorization", "Basic abc")]).await;
    assert_eq!(no_bearer.json()["message"], "missing bearer token");

    let unknown_kid = get(mint(
        &foreign_key(),
        "kid-unknown",
        &jwks.discovery_issuer,
        &Claims::new("prn_1", "CLIENT", "x", &[]),
    ))
    .await;
    assert_eq!(unknown_kid.status, 401);
    assert_eq!(
        jwks.jwks_requests(),
        1,
        "an unknown kid inside 30 s does not refetch"
    );
    assert_eq!(h.probes().total_invocations(), 1);

    // past the floor, an unknown kid refetches and the rotation is picked up
    h.clock.advance(chrono::Duration::seconds(31));
    jwks.rotate();
    let rotated = get(jwks.mint(&Claims::new("prn_1", "CLIENT", "x", &[]))).await;
    assert_eq!(rotated.status, 200, "{}", rotated.text());
    assert_eq!(jwks.jwks_requests(), 2);
}

/// The platform issues `clients` / `applications` as `"{id}:{label}"` pairs
/// (or `*`); the function sees bare ids, and an identity token is no API
/// credential.
#[tokio::test]
async fn h4_scope_pairs_are_ids_and_identity_tokens_are_refused() {
    let jwks = TestJwks::start().await;
    let h = bearer_harness(&jwks).await;
    let path = fpath(ADDR, "/api/x");
    let get = |token: String| {
        let h = &h;
        let path = path.clone();
        async move {
            h.get(&path, &[("Authorization", &format!("Bearer {token}"))])
                .await
        }
    };

    let paired = get(jwks.mint(
        &Claims::new("prn_1", "CLIENT", "x", &["clt_1:acme"])
            .applications(&["app_1:orders", "app_2:billing"], false)
            .token_use("api"),
    ))
    .await;
    assert_eq!(paired.status, 200, "{}", paired.text());
    let caller = &paired.json()["caller"];
    assert_eq!(caller["clientId"], "clt_1");
    assert_eq!(caller["clients"], json!(["clt_1"]));
    assert_eq!(caller["applications"], json!(["app_1", "app_2"]));
    assert_eq!(caller["allApplications"], false);

    let wildcard =
        get(jwks.mint(&Claims::new("prn_1", "PARTNER", "x", &["*"]).applications(&["*"], false)))
            .await;
    assert_eq!(wildcard.status, 200, "{}", wildcard.text());
    assert_eq!(wildcard.json()["caller"]["clients"], json!(["*"]));
    assert_eq!(wildcard.json()["caller"]["applications"], json!([]));
    assert_eq!(wildcard.json()["caller"]["allApplications"], true);

    let identity =
        get(jwks.mint(&Claims::new("prn_1", "ANCHOR", "x", &[]).token_use("identity"))).await;
    assert_eq!(identity.status, 401);
    assert_eq!(
        identity.json()["message"],
        "an identity token is not an API credential"
    );
}

#[tokio::test]
async fn h4_every_claim_reaches_the_principal() {
    let jwks = TestJwks::start().await;
    let h = bearer_harness(&jwks).await;
    let token = jwks.mint(
        &Claims::new(
            "prn_claims",
            "CLIENT",
            "platform:function:function:view  other:perm",
            &["clt_1", "clt_2"],
        )
        .roles(&["role-a", "role-b"])
        .applications(&["app_1"], false),
    );
    let ok = h
        .get(
            &fpath(ADDR, "/api/x"),
            &[("Authorization", &format!("Bearer {token}"))],
        )
        .await;
    assert_eq!(ok.status, 200, "{}", ok.text());
    let caller = &ok.json()["caller"];
    assert_eq!(caller["tier"], "CLIENT");
    assert_eq!(
        caller["clientId"],
        json!(null),
        "two clients: none is unambiguous"
    );
    assert_eq!(caller["clients"], json!(["clt_1", "clt_2"]));
    assert_eq!(caller["roles"], json!(["role-a", "role-b"]));
    assert_eq!(caller["applications"], json!(["app_1"]));
    assert_eq!(caller["allApplications"], false);
    assert_eq!(
        caller["permissions"],
        json!(["other:perm", "platform:function:function:view"])
    );
}

#[tokio::test]
async fn h4_a_foreign_jwks_uri_is_not_followed() {
    let jwks = TestJwks::start().await;
    let decoy = axum::Router::new().route(
        "/.well-known/jwks.json",
        axum::routing::get(|| async { r#"{"keys":[]}"# }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let decoy_url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    tokio::spawn(async move { axum::serve(listener, decoy).await.unwrap() });
    jwks.use_foreign_jwks_uri(&format!("{decoy_url}/.well-known/jwks.json"));
    let h = bearer_harness(&jwks).await;
    let token = jwks.mint(&Claims::new("prn_1", "CLIENT", "x", &[]));
    let ok = h
        .get(
            &fpath(ADDR, "/api/x"),
            &[("Authorization", &format!("Bearer {token}"))],
        )
        .await;
    assert_eq!(ok.status, 200, "{}", ok.text());
    assert_eq!(
        jwks.jwks_requests(),
        1,
        "keys came from the platform URL's own jwks.json"
    );
}

#[tokio::test]
async fn h4_discovery_down_then_up() {
    let jwks = TestJwks::start().await;
    jwks.break_discovery();
    let h = bearer_harness(&jwks).await;
    let token = jwks.mint(&Claims::new("prn_1", "CLIENT", "x", &[]));
    let auth = format!("Bearer {token}");
    let down = h
        .get(&fpath(ADDR, "/api/x"), &[("Authorization", &auth)])
        .await;
    assert_eq!(down.status, 401);
    assert_eq!(
        down.json()["message"],
        "platform issuer could not be discovered"
    );
    assert_eq!(down.header("www-authenticate").as_deref(), Some("Bearer"));
    let still_down = h
        .get(&fpath(ADDR, "/api/x"), &[("Authorization", &auth)])
        .await;
    assert_eq!(still_down.status, 401);
    assert_eq!(
        jwks.discovery_requests(),
        1,
        "floor-gated like the JWKS fetch"
    );

    jwks.fix_discovery();
    h.clock.advance(chrono::Duration::seconds(31));
    let up = h
        .get(&fpath(ADDR, "/api/x"), &[("Authorization", &auth)])
        .await;
    assert_eq!(up.status, 200, "{}", up.text());
}

// ── H5: header stripping by auth mode ────────────────────────────────────

#[tokio::test]
async fn h5_header_stripping_by_auth_mode() {
    let h = Harness::start(doc(vec![with(
        entry(
            ADDR,
            1,
            "live",
            "echo",
            10,
            json!([{"path": "/none/*", "auth": "none"}, {"path": "/events/*", "auth": "webhook"}]),
        ),
        json!({"webhookSigningSecret": "h5"}),
    )]))
    .await;
    let none = h
        .get(
            &fpath(ADDR, "/none/x"),
            &[
                ("Authorization", "Bearer keepme"),
                ("X-FlowCatalyst-Signature", "sigkeepme"),
                ("X-FlowCatalyst-Timestamp", "tskeepme"),
                ("X-FlowCatalyst-Function", "a.b.c"),
            ],
        )
        .await;
    let headers = &none.json()["headers"];
    assert_eq!(headers["authorization"], json!(["Bearer keepme"]));
    assert_eq!(headers["x-flowcatalyst-signature"], json!(["sigkeepme"]));
    assert_eq!(headers["x-flowcatalyst-timestamp"], json!(["tskeepme"]));
    assert!(
        headers.get("x-flowcatalyst-function").is_none(),
        "always stripped"
    );

    let ts = timestamp(h.clock.now());
    let sig = signed("h5", &ts, b"{}");
    let webhook = h
        .post(
            &fpath(ADDR, "/events/x"),
            b"{}",
            &[
                ("Authorization", "Bearer strip-me"),
                ("X-FlowCatalyst-Signature", &sig),
                ("X-FlowCatalyst-Timestamp", &ts),
            ],
        )
        .await;
    assert_eq!(webhook.status, 200, "{}", webhook.text());
    let headers = &webhook.json()["headers"];
    for consumed in [
        "authorization",
        "x-flowcatalyst-signature",
        "x-flowcatalyst-timestamp",
    ] {
        assert!(headers.get(consumed).is_none(), "{consumed} was consumed");
    }
}

// ── H6: permits ──────────────────────────────────────────────────────────

#[tokio::test]
async fn h6_per_function_permits() {
    let h = Arc::new(
        Harness::start(doc(vec![entry(
            ADDR,
            1,
            "live",
            "park",
            1,
            json!([{"path": "/*", "auth": "none"}]),
        )]))
        .await,
    );
    let first = {
        let h = h.clone();
        tokio::spawn(async move { h.get(&fpath(ADDR, "/park"), &[]).await })
    };
    h.probes().await_started(1).await;

    let second = h.get(&fpath(ADDR, "/park"), &[]).await;
    assert_eq!(second.status, 429);
    assert_eq!(second.header("retry-after").as_deref(), Some("1"));
    assert_eq!(
        second.text(),
        r#"{"error":"BUSY","message":"the function is at capacity"}"#
    );
    assert_eq!(h.function_permits(ADDR), Some(0));
    assert_eq!(
        h.host_permits(),
        511,
        "the refused call gave its host permit back"
    );
    assert_eq!(
        h.probes().started.load(Ordering::SeqCst),
        1,
        "never entered"
    );

    h.probes().release_one();
    assert_eq!(first.await.unwrap().status, 200);

    let third = {
        let h = h.clone();
        tokio::spawn(async move { h.get(&fpath(ADDR, "/park"), &[]).await })
    };
    h.probes().await_started(2).await;
    h.probes().release_one();
    assert_eq!(third.await.unwrap().status, 200);
    assert_eq!(h.function_permits(ADDR), Some(1));
    assert_eq!(h.host_permits(), 512);
}

#[tokio::test]
async fn h6_the_host_permit_is_shared_across_functions() {
    let h = Arc::new(
        Harness::start_with(
            doc(vec![
                entry(
                    ADDR,
                    1,
                    "live",
                    "park",
                    5,
                    json!([{"path": "/*", "auth": "none"}]),
                ),
                entry(
                    ADDR_B,
                    1,
                    "live",
                    "echo",
                    5,
                    json!([{"path": "/*", "auth": "none"}]),
                ),
            ]),
            Options {
                max_concurrency: 1,
                ..Options::default()
            },
        )
        .await,
    );
    let parked = {
        let h = h.clone();
        tokio::spawn(async move { h.get(&fpath(ADDR, "/x"), &[]).await })
    };
    h.probes().await_started(1).await;
    let other = h.get(&fpath(ADDR_B, "/x"), &[]).await;
    assert_eq!(other.status, 429);
    assert_eq!(h.probes().invocations(&format!("{ADDR_B}@1")), 0);
    h.probes().release_one();
    assert_eq!(parked.await.unwrap().status, 200);
    assert_eq!(h.get(&fpath(ADDR_B, "/x"), &[]).await.status, 200);
    assert_eq!(h.host_permits(), 1);
}

// ── H7: the deadline ─────────────────────────────────────────────────────

#[tokio::test]
async fn h7_timeout_answers_504_and_keeps_the_permit_until_the_worker_returns() {
    let h = Harness::start(doc(vec![entry(
        ADDR,
        1,
        "live",
        "park",
        1,
        json!([{"path": "/*", "auth": "none", "timeoutMs": 200}]),
    )]))
    .await;
    let started = Instant::now();
    let resp = h.get(&fpath(ADDR, "/park"), &[]).await;
    assert_eq!(resp.status, 504);
    assert_eq!(
        resp.text(),
        r#"{"error":"FUNCTION_TIMEOUT","message":"the invocation exceeded its deadline"}"#
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the deadline ends the response"
    );
    assert_eq!(
        h.function_permits(ADDR),
        Some(0),
        "a function that ignores the interrupt keeps its permit"
    );
    assert!(h
        .scrape()
        .contains(&format!("fc_fn_active{{address=\"{ADDR}\"}} 1")));

    h.probes().release_one();
    wait_until(|| h.function_permits(ADDR) == Some(1)).await;
    assert!(h
        .scrape()
        .contains(&format!("fc_fn_active{{address=\"{ADDR}\"}} 0")));
}

#[tokio::test]
async fn h7_an_interruptible_function_sees_the_interrupt() {
    let h = Harness::start(doc(vec![entry(
        ADDR,
        1,
        "live",
        "park-interruptible",
        1,
        json!([{"path": "/*", "auth": "none", "timeoutMs": 100}]),
    )]))
    .await;
    assert_eq!(h.get(&fpath(ADDR, "/park"), &[]).await.status, 504);
    wait_until(|| h.probes().interrupted.load(Ordering::SeqCst) == 1).await;
    wait_until(|| h.function_permits(ADDR) == Some(1)).await;
}

// ── H8: failures never leak ──────────────────────────────────────────────

#[tokio::test]
async fn h8_failed_panicked_unavailable_and_runtime_timeouts() {
    let h = Harness::start(doc(vec![
        entry(
            "app.fail.fail",
            1,
            "live",
            "fail",
            5,
            json!([{"path": "/*", "auth": "none"}]),
        ),
        entry(
            "app.fail.panic",
            1,
            "live",
            "panic",
            5,
            json!([{"path": "/*", "auth": "none"}]),
        ),
        entry(
            "app.fail.gone",
            1,
            "live",
            "unavailable",
            5,
            json!([{"path": "/*", "auth": "none"}]),
        ),
        entry(
            "app.fail.slow",
            1,
            "live",
            "timeout",
            5,
            json!([{"path": "/*", "auth": "none"}]),
        ),
    ]))
    .await;
    for address in ["app.fail.fail", "app.fail.panic"] {
        let resp = h.get(&fpath(address, "/x"), &[]).await;
        assert_eq!(resp.status, 500, "{address}");
        assert_eq!(
            resp.text(),
            r#"{"error":"FUNCTION_ERROR","message":"the function failed"}"#
        );
        assert_eq!(
            h.function_permits(address),
            Some(5),
            "{address}: permits released"
        );
    }
    let gone = h.get(&fpath("app.fail.gone", "/x"), &[]).await;
    assert_eq!(gone.status, 503);
    assert_eq!(gone.header("retry-after").as_deref(), Some("15"));
    assert_eq!(gone.error(), "FUNCTION_UNAVAILABLE");
    let slow = h.get(&fpath("app.fail.slow", "/x"), &[]).await;
    assert_eq!(slow.status, 504);
    assert_eq!(slow.error(), "FUNCTION_TIMEOUT");
    let scrape = h.scrape();
    for (address, outcome) in [
        ("app.fail.fail", "error"),
        ("app.fail.panic", "error"),
        ("app.fail.gone", "unavailable"),
        ("app.fail.slow", "timeout"),
    ] {
        assert!(
            scrape.contains(&format!(
                "fc_fn_invocations_total{{address=\"{address}\",version=\"1\",outcome=\"{outcome}\",entry=\"private\"}} 1"
            )),
            "{address} {outcome}:\n{scrape}"
        );
    }
}

// ── H9: body cap ─────────────────────────────────────────────────────────

#[tokio::test]
async fn h9_body_cap() {
    let h = Harness::start(doc(vec![entry(
        ADDR,
        1,
        "live",
        "echo",
        5,
        json!([{"path": "/*", "auth": "none", "maxBodyBytes": 10}]),
    )]))
    .await;
    let over = h.post(&fpath(ADDR, "/x"), b"0123456789X", &[]).await;
    assert_eq!(over.status, 413);
    assert_eq!(
        over.text(),
        r#"{"error":"BODY_TOO_LARGE","message":"request body exceeds the endpoint's limit"}"#
    );

    let chunked = h
        .send(
            h.client
                .post(format!("{}{}", h.base, fpath(ADDR, "/x")))
                .body(reqwest::Body::wrap(http_body_util::StreamBody::new(
                    futures::stream::iter(vec![
                        Ok::<_, std::io::Error>(hyper::body::Frame::data(
                            bytes::Bytes::from_static(b"01234"),
                        )),
                        Ok(hyper::body::Frame::data(bytes::Bytes::from_static(
                            b"56789X",
                        ))),
                    ]),
                ))),
        )
        .await;
    assert_eq!(chunked.status, 413);

    // declared over the cap: refused before a single body byte is sent
    let port = h.listener.port().unwrap();
    let declared = raw(
        port,
        "POST",
        &fpath(ADDR, "/x"),
        &[("Host", "127.0.0.1"), ("Content-Length", "1000000")],
        b"",
    )
    .await;
    assert_eq!(declared.status, 413);
    assert_eq!(h.probes().total_invocations(), 0);

    let at_cap = h.post(&fpath(ADDR, "/x"), b"0123456789", &[]).await;
    assert_eq!(at_cap.status, 200);
    assert_eq!(at_cap.json()["bodyLength"], 10);
    assert_eq!(h.probes().total_invocations(), 1);
}

// ── H10: the response ────────────────────────────────────────────────────

#[tokio::test]
async fn h10_hop_by_hop_and_content_length_from_the_function_are_dropped() {
    let h = Harness::start(doc(vec![entry(
        ADDR,
        1,
        "live",
        "echo",
        5,
        json!([{"path": "/*", "auth": "none"}]),
    )]))
    .await;
    let resp = h.get(&fpath(ADDR, "/x?status=201"), &[]).await;
    assert_eq!(resp.status, 201);
    assert_eq!(resp.headers_all("x-multi"), ["a", "b"]);
    assert_eq!(resp.header("connection"), None);
    assert_eq!(
        resp.header("content-length")
            .unwrap()
            .parse::<usize>()
            .unwrap(),
        resp.body.len()
    );
    assert_ne!(resp.body.len(), 999);
}

// ── H11: versioned calls ─────────────────────────────────────────────────

fn versioned_document() -> serde_json::Value {
    doc(vec![
        with(
            entry(
                ADDR,
                1,
                "live",
                "echo",
                5,
                json!([{"path": "/events/*", "auth": "webhook"}, {"path": "/x", "auth": "none"}]),
            ),
            json!({"webhookSigningSecret": "wh", "clientId": "clt_1", "applicationId": "app_1"}),
        ),
        with(
            entry(
                ADDR,
                2,
                "candidate",
                "echo",
                5,
                json!([{"path": "/x", "auth": "none"}]),
            ),
            json!({"clientId": "clt_1", "applicationId": "app_1"}),
        ),
        with(
            entry(
                "platform.core.fn",
                1,
                "live",
                "echo",
                5,
                json!([{"path": "/x", "auth": "none"}]),
            ),
            json!({"applicationId": "app_p"}),
        ),
    ])
}

/// Owner ruling 12 (Java 8a130505): past the token, permission and reach
/// checks, a pinned version still being prepared answers 503
/// `VERSION_NOT_READY` with `Retry-After: 5`; refused for good it stays 404
/// `VERSION_NOT_AVAILABLE`.
#[tokio::test]
async fn h11_a_pinned_version_still_preparing_is_503_until_ready() {
    let jwks = TestJwks::start().await;
    let h = Harness::start_with(
        versioned_document(),
        Options {
            platform_url: Some(jwks.url.clone()),
            ..Options::default()
        },
    )
    .await;
    let bearer = |claims: Claims| format!("Bearer {}", jwks.mint(&claims));
    let invoke = "platform:function:version:invoke";
    let with_perm = bearer(Claims::new("prn_v", "CLIENT", invoke, &["clt_1"]));
    let other_client = bearer(Claims::new("prn_o", "CLIENT", invoke, &["clt_OTHER"]));

    let candidate = |version: i32| {
        with(
            entry(
                ADDR,
                version,
                "candidate",
                "echo",
                5,
                json!([{"path": "/x", "auth": "none"}]),
            ),
            json!({"clientId": "clt_1", "applicationId": "app_1"}),
        )
    };
    let mut document = versioned_document();
    let functions = document["functions"].as_array_mut().unwrap();
    functions.push(candidate(3));
    functions.push(candidate(4));
    h.store.hold(&format!("mem://{ADDR}/3"));
    h.store.fail(
        &format!("mem://{ADDR}/4"),
        fc_fnhost_core::artifact::ArtifactError::DigestMismatch {
            expected: support::fakes::digest_for(ADDR, 4),
            actual: support::fakes::digest_for("x", 0),
        },
    );
    h.control.serve(support::fakes::Answer::Document(
        support::fakes::document_json(document),
    ));
    let cycle = {
        let reconciler = h.reconciler.clone();
        let now = h.clock.now();
        tokio::spawn(async move { reconciler.reconcile_once(now).await })
    };
    let address = fc_function_abi::FunctionAddress::parse(ADDR).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while h
        .reconciler
        .document()
        .and_then(|d| d.entry_for(&address, 4).map(|_| ()))
        .is_none()
    {
        assert!(
            Instant::now() < deadline,
            "the reconcile never began preparing"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    let preparing = h
        .get(&fpath(ADDR, ":3/x"), &[("Authorization", &with_perm)])
        .await;
    assert_eq!(preparing.status, 503, "{}", preparing.text());
    assert_eq!(
        preparing.text(),
        r#"{"error":"VERSION_NOT_READY","message":"the version is still being prepared"}"#
    );
    assert_eq!(preparing.header("retry-after").as_deref(), Some("5"));
    // Only past the reach checks: nothing leaks to a caller out of reach.
    let out_of_reach = h
        .get(&fpath(ADDR, ":3/x"), &[("Authorization", &other_client)])
        .await;
    assert_eq!(out_of_reach.status, 404);
    assert_eq!(h.get(&fpath(ADDR, ":3/x"), &[]).await.status, 401);

    h.store.release(&format!("mem://{ADDR}/3"));
    cycle.await.unwrap();
    let ready = h
        .get(&fpath(ADDR, ":3/x"), &[("Authorization", &with_perm)])
        .await;
    assert_eq!(ready.status, 200, "{}", ready.text());
    assert_eq!(ready.json()["label"], format!("{ADDR}@3"));

    let refused = h
        .get(&fpath(ADDR, ":4/x"), &[("Authorization", &with_perm)])
        .await;
    assert_eq!(refused.status, 404, "{}", refused.text());
    assert_eq!(refused.error(), "VERSION_NOT_AVAILABLE");
}

#[tokio::test]
async fn h11_versioned_invoke() {
    let jwks = TestJwks::start().await;
    let h = Harness::start_with(
        versioned_document(),
        Options {
            platform_url: Some(jwks.url.clone()),
            ..Options::default()
        },
    )
    .await;
    let bearer = |claims: Claims| format!("Bearer {}", jwks.mint(&claims));
    let invoke = "platform:function:version:invoke";
    let with_perm = bearer(Claims::new(
        "prn_v",
        "CLIENT",
        "platform:*:version:invoke",
        &["clt_1"],
    ));
    let no_perm = bearer(Claims::new(
        "prn_v",
        "CLIENT",
        "platform:function:function:view",
        &["clt_1"],
    ));
    let anchor = bearer(Claims::new("prn_a", "ANCHOR", invoke, &[]));
    let other_client = bearer(Claims::new("prn_o", "CLIENT", invoke, &["clt_OTHER"]));
    let restricted_app = bearer(
        Claims::new("prn_r", "CLIENT", invoke, &["clt_1"]).applications(&["app_OTHER"], false),
    );
    let allowed_app =
        bearer(Claims::new("prn_r", "CLIENT", invoke, &["clt_1"]).applications(&["app_1"], false));

    let no_token = h.get(&fpath(ADDR, ":2/x"), &[]).await;
    assert_eq!(no_token.status, 401);
    assert_eq!(
        no_token.header("www-authenticate").as_deref(),
        Some("Bearer")
    );
    let forbidden = h
        .get(&fpath(ADDR, ":2/x"), &[("Authorization", &no_perm)])
        .await;
    assert_eq!(forbidden.status, 403);
    assert_eq!(
        forbidden.text(),
        r#"{"error":"PERMISSION_REQUIRED","message":"platform:function:version:invoke required"}"#
    );
    for (token, why) in [
        (&other_client, "another client"),
        (&restricted_app, "another application"),
    ] {
        let resp = h
            .get(&fpath(ADDR, ":2/x"), &[("Authorization", token)])
            .await;
        assert_eq!(resp.status, 404, "{why}");
        assert_eq!(
            resp.text(),
            r#"{"error":"VERSION_NOT_AVAILABLE","message":"no such version"}"#
        );
    }
    let unknown = h
        .get(&fpath(ADDR, ":99/x"), &[("Authorization", &with_perm)])
        .await;
    assert_eq!(unknown.status, 404);
    assert_eq!(unknown.error(), "VERSION_NOT_AVAILABLE");
    let platform_owned = h
        .get(
            &fpath("platform.core.fn", ":1/x"),
            &[("Authorization", &with_perm)],
        )
        .await;
    assert_eq!(
        platform_owned.status, 404,
        "a platform-owned function needs anchor"
    );
    assert_eq!(
        h.get(
            &fpath("platform.core.fn", ":1/x"),
            &[("Authorization", &anchor)]
        )
        .await
        .status,
        200
    );

    let candidate = h
        .get(&fpath(ADDR, ":2/x"), &[("Authorization", &with_perm)])
        .await;
    assert_eq!(candidate.status, 200, "{}", candidate.text());
    assert_eq!(candidate.json()["label"], format!("{ADDR}@2"));
    assert_eq!(candidate.json()["caller"]["kind"], "Principal");
    assert!(candidate.json()["headers"].get("authorization").is_none());
    let unversioned = h.get(&fpath(ADDR, "/x"), &[]).await;
    assert_eq!(
        unversioned.json()["label"],
        format!("{ADDR}@1"),
        "unversioned is live"
    );
    assert_eq!(
        h.get(&fpath(ADDR, ":2/x"), &[("Authorization", &anchor)])
            .await
            .status,
        200,
        "anchor reaches regardless of client scope"
    );
    assert_eq!(
        h.get(&fpath(ADDR, ":2/x"), &[("Authorization", &allowed_app)])
            .await
            .status,
        200
    );
    // the pair form the platform actually issues, and its wildcards
    for (claims, why) in [
        (
            Claims::new("prn_p", "CLIENT", invoke, &["clt_1:acme"])
                .applications(&["app_1:orders"], false),
            "pairs",
        ),
        (
            Claims::new("prn_w", "PARTNER", invoke, &["*"]).applications(&["*"], false),
            "wildcards",
        ),
    ] {
        let resp = h
            .get(&fpath(ADDR, ":2/x"), &[("Authorization", &bearer(claims))])
            .await;
        assert_eq!(resp.status, 200, "{why}: {}", resp.text());
    }
    let other_pair = bearer(
        Claims::new("prn_p", "CLIENT", invoke, &["clt_1:acme"])
            .applications(&["app_OTHER:x"], false),
    );
    assert_eq!(
        h.get(&fpath(ADDR, ":2/x"), &[("Authorization", &other_pair)])
            .await
            .status,
        404
    );

    // a webhook endpoint is reachable versioned without a signature
    let versioned_webhook = h
        .post(
            &fpath(ADDR, ":1/events/x"),
            b"{}",
            &[("Authorization", &with_perm)],
        )
        .await;
    assert_eq!(
        versioned_webhook.status,
        200,
        "{}",
        versioned_webhook.text()
    );
    assert_eq!(versioned_webhook.json()["caller"]["kind"], "Principal");
    assert_eq!(
        h.loader.load_count(&format!("{ADDR}@2")),
        1,
        "pinned once, reused"
    );

    // the endpoint's own cap still applies after authentication
    let versioned_405 = h
        .post(
            &fpath(ADDR, ":2/x"),
            b"{}",
            &[("Authorization", &with_perm)],
        )
        .await;
    assert_eq!(
        versioned_405.status, 200,
        "no methods declared: every method"
    );
    let missing = h
        .get(&fpath(ADDR, ":2/nope"), &[("Authorization", &with_perm)])
        .await;
    assert_eq!(missing.error(), "ENDPOINT_NOT_FOUND");
}

#[tokio::test]
async fn h11b_versioned_refusals_are_indistinguishable_for_an_existing_or_unknown_version() {
    let jwks = TestJwks::start().await;
    let h = Harness::start_with(
        versioned_document(),
        Options {
            platform_url: Some(jwks.url.clone()),
            ..Options::default()
        },
    )
    .await;
    let existing = h.get(&fpath(ADDR, ":1/x"), &[]).await;
    let missing = h.get(&fpath(ADDR, ":99/x"), &[]).await;
    assert_eq!((existing.status, missing.status), (401, 401));
    assert_eq!(existing.body, missing.body);

    let no_perm = format!(
        "Bearer {}",
        jwks.mint(&Claims::new(
            "prn_v",
            "CLIENT",
            "platform:function:function:view",
            &["clt_1"]
        ))
    );
    let existing = h
        .get(&fpath(ADDR, ":1/x"), &[("Authorization", &no_perm)])
        .await;
    let missing = h
        .get(&fpath(ADDR, ":99/x"), &[("Authorization", &no_perm)])
        .await;
    assert_eq!((existing.status, missing.status), (403, 403));
    assert_eq!(existing.body, missing.body);
    assert_eq!(h.probes().total_invocations(), 0);
    let scrape = h.scrape();
    assert!(
        scrape.contains(
            "fc_fn_invocations_total{address=\"-\",version=\"-\",outcome=\"unauthorized\",entry=\"private\"} 4"
        ),
        "versioned refusals carry no address:\n{scrape}"
    );
}

#[tokio::test]
async fn a_versioned_body_over_the_endpoint_cap_is_413_after_authentication() {
    let jwks = TestJwks::start().await;
    let h = Harness::start_with(
        doc(vec![with(
            entry(
                ADDR,
                1,
                "live",
                "echo",
                5,
                json!([{"path": "/x", "auth": "none", "maxBodyBytes": 4}]),
            ),
            json!({"clientId": "clt_1"}),
        )]),
        Options {
            platform_url: Some(jwks.url.clone()),
            ..Options::default()
        },
    )
    .await;
    let token = format!(
        "Bearer {}",
        jwks.mint(&Claims::new(
            "p",
            "ANCHOR",
            "platform:function:version:invoke",
            &[]
        ))
    );
    assert_eq!(
        h.post(&fpath(ADDR, ":1/x"), b"12345", &[]).await.status,
        401
    );
    let over = h
        .post(&fpath(ADDR, ":1/x"), b"12345", &[("Authorization", &token)])
        .await;
    assert_eq!(over.status, 413);
    let huge = vec![b'x'; 1_048_577];
    assert_eq!(
        h.post(&fpath(ADDR, ":1/x"), &huge, &[]).await.status,
        413,
        "the fixed pre-auth cap"
    );
}

#[tokio::test]
async fn pinned_versions_are_closed_once_their_version_leaves_desired_state() {
    let jwks = TestJwks::start().await;
    let live = with(
        entry(
            ADDR,
            1,
            "live",
            "echo",
            5,
            json!([{"path": "/x", "auth": "none"}]),
        ),
        json!({"clientId": "clt_1"}),
    );
    let candidate = with(
        entry(
            ADDR,
            2,
            "candidate",
            "echo",
            5,
            json!([{"path": "/x", "auth": "none"}]),
        ),
        json!({"clientId": "clt_1"}),
    );
    let h = Harness::start_with(
        doc(vec![live.clone(), candidate]),
        Options {
            platform_url: Some(jwks.url.clone()),
            ..Options::default()
        },
    )
    .await;
    let token = format!(
        "Bearer {}",
        jwks.mint(&Claims::new(
            "p",
            "CLIENT",
            "platform:function:version:invoke",
            &["clt_1"]
        ))
    );
    assert_eq!(
        h.get(&fpath(ADDR, ":2/x"), &[("Authorization", &token)])
            .await
            .status,
        200
    );
    let pinned = h.loader.instance(&format!("{ADDR}@2"));
    assert_eq!(h.listener.pinned_versions().unwrap().len(), 1);
    assert!(!pinned.closed.load(Ordering::SeqCst));

    h.publish(doc(vec![live])).await;
    wait_until(|| pinned.closed.load(Ordering::SeqCst)).await;
    assert!(h.listener.pinned_versions().unwrap().is_empty());
}

// ── H12: lazy loading ────────────────────────────────────────────────────

#[tokio::test]
async fn h12_lazy_load_on_first_call_once_under_concurrent_first_calls() {
    let mut lazy = entry(
        ADDR,
        1,
        "live",
        "echo",
        5,
        json!([{"path": "/*", "auth": "none"}]),
    );
    lazy["mode"] = json!("lazy");
    let h = Arc::new(Harness::start(doc(vec![lazy])).await);
    let label = format!("{ADDR}@1");
    assert_eq!(
        h.loader.load_count(&label),
        0,
        "lazy: nothing loaded by the reconcile"
    );
    *h.loader.delay.lock() = Some(Duration::from_millis(100));
    let calls: Vec<_> = (0..2)
        .map(|_| {
            let h = h.clone();
            tokio::spawn(async move { h.get(&fpath(ADDR, "/x"), &[]).await.status })
        })
        .collect();
    for call in calls {
        assert_eq!(call.await.unwrap(), 200);
    }
    assert_eq!(h.loader.load_count(&label), 1);
}

#[tokio::test]
async fn h12_an_unloadable_function_is_503_not_404() {
    let mut lazy = entry(
        ADDR,
        1,
        "live",
        "echo",
        5,
        json!([{"path": "/*", "auth": "none"}]),
    );
    lazy["mode"] = json!("lazy");
    let h = Harness::start(doc(vec![])).await;
    h.loader.refusals.lock().push(format!("{ADDR}@1"));
    h.publish(doc(vec![lazy])).await;
    let resp = h.get(&fpath(ADDR, "/x"), &[]).await;
    assert_eq!(resp.status, 503);
    assert_eq!(resp.header("retry-after").as_deref(), Some("15"));
    assert_eq!(
        resp.text(),
        r#"{"error":"FUNCTION_UNAVAILABLE","message":"the function could not be loaded"}"#
    );
    assert_eq!(h.function_permits(ADDR), Some(5));
}

// ── H13: promote while a call is parked ──────────────────────────────────

#[tokio::test]
async fn h13_promote_while_a_call_is_parked_on_the_old_version() {
    let v1 = entry(
        ADDR,
        1,
        "live",
        "park",
        5,
        json!([{"path": "/*", "auth": "none"}]),
    );
    let v2 = entry(
        ADDR,
        2,
        "live",
        "echo",
        5,
        json!([{"path": "/*", "auth": "none"}]),
    );
    let h = Arc::new(Harness::start(doc(vec![v1])).await);
    let parked = {
        let h = h.clone();
        tokio::spawn(async move { h.get(&fpath(ADDR, "/x"), &[]).await })
    };
    h.probes().await_started(1).await;
    let publish = {
        let h = h.clone();
        tokio::spawn(async move { h.publish(doc(vec![v2])).await })
    };
    wait_until(|| h.loader.load_count(&format!("{ADDR}@2")) == 1).await;
    let new_call = h.get(&fpath(ADDR, "/x"), &[]).await;
    assert_eq!(
        new_call.json()["label"],
        format!("{ADDR}@2"),
        "new calls get v2"
    );
    let old = h.loader.instance(&format!("{ADDR}@1"));
    assert!(
        !old.closed.load(Ordering::SeqCst),
        "v1 stays open while its call is parked"
    );

    h.probes().release_one();
    let parked = parked.await.unwrap();
    assert_eq!(
        parked.json()["tag"],
        format!("{ADDR}@1"),
        "the parked call completes on v1"
    );
    publish.await.unwrap();
    wait_until(|| old.closed.load(Ordering::SeqCst)).await;
    assert!(!old.closed_while_running.load(Ordering::SeqCst));
}

// ── H14: drain ───────────────────────────────────────────────────────────

#[tokio::test]
async fn h14_drain_rejects_new_requests_and_lets_in_flight_finish() {
    let h = Arc::new(
        Harness::start(doc(vec![entry(
            ADDR,
            1,
            "live",
            "park",
            5,
            json!([{"path": "/*", "auth": "none"}]),
        )]))
        .await,
    );
    let in_flight = {
        let h = h.clone();
        tokio::spawn(async move { h.get(&fpath(ADDR, "/x"), &[]).await })
    };
    h.probes().await_started(1).await;
    h.listener.drain().await;
    let refused = h.get(&fpath(ADDR, "/x"), &[]).await;
    assert_eq!(refused.status, 503);
    assert_eq!(refused.header("retry-after").as_deref(), Some("5"));
    assert_eq!(
        refused.text(),
        r#"{"error":"DRAINING","message":"the host is draining"}"#
    );
    let closing = {
        let h = h.clone();
        tokio::spawn(async move { h.listener.close(Duration::from_secs(10)).await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    h.probes().release_one();
    assert_eq!(
        in_flight.await.unwrap().status,
        200,
        "in flight completes during drain"
    );
    tokio::time::timeout(Duration::from_secs(10), closing)
        .await
        .expect("close returns once in-flight work is done")
        .unwrap();
}

#[tokio::test]
async fn h14_close_returns_within_the_drain_timeout_even_if_a_function_never_returns() {
    let h = Arc::new(
        Harness::start(doc(vec![entry(
            ADDR,
            1,
            "live",
            "park",
            5,
            json!([{"path": "/*", "auth": "none"}]),
        )]))
        .await,
    );
    let _never = {
        let h = h.clone();
        tokio::spawn(async move {
            let _ = h
                .client
                .get(format!("{}{}", h.base, fpath(ADDR, "/x")))
                .send()
                .await;
        })
    };
    h.probes().await_started(1).await;
    let started = Instant::now();
    h.close(Duration::from_millis(300)).await;
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert!(!h.listener.is_serving());
}

// ── h2c, metrics ────────────────────────────────────────────────

#[tokio::test]
async fn h2c_prior_knowledge_is_served() {
    let h = Harness::start(doc(vec![entry(
        ADDR,
        1,
        "live",
        "echo",
        5,
        json!([{"path": "/*", "auth": "none"}]),
    )]))
    .await;
    let client = reqwest::Client::builder()
        .http2_prior_knowledge()
        .build()
        .unwrap();
    let resp = client
        .get(format!("{}{}", h.base, fpath(ADDR, "/x")))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.version(), reqwest::Version::HTTP_2);
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body["originalHost"]
            .as_str()
            .unwrap()
            .starts_with("127.0.0.1:"),
        "{body}"
    );
}

#[tokio::test]
async fn observer_outcomes_and_gauges() {
    let h = Harness::start(doc(vec![with(
        entry(
            ADDR,
            1,
            "live",
            "echo",
            5,
            json!([{"path": "/*", "auth": "none"}, {"path": "/hook", "auth": "webhook"}]),
        ),
        json!({"webhookSigningSecret": "s"}),
    )]))
    .await;
    for status in ["200", "404", "429", "500", "302"] {
        h.get(&fpath(ADDR, &format!("/x?status={status}")), &[])
            .await;
    }
    h.post(&fpath(ADDR, "/hook"), b"{}", &[]).await;
    h.get(&fpath("app.orders.unknown", "/x"), &[]).await;
    let scrape = h.scrape();
    for (outcome, count) in [("ok", 2), ("client_error", 1), ("retry", 1), ("error", 1)] {
        assert!(
            scrape.contains(&format!(
                "fc_fn_invocations_total{{address=\"{ADDR}\",version=\"1\",outcome=\"{outcome}\",entry=\"private\"}} {count}"
            )),
            "{outcome}:\n{scrape}"
        );
    }
    assert!(scrape.contains(&format!(
        "fc_fn_invocations_total{{address=\"{ADDR}\",version=\"-\",outcome=\"unauthorized\",entry=\"private\"}} 1"
    )));
    assert!(scrape.contains(
        "fc_fn_invocations_total{address=\"-\",version=\"-\",outcome=\"not_found\",entry=\"private\"} 1"
    ));
    assert!(scrape.contains(&format!(
        "fc_fn_duration_seconds_count{{address=\"{ADDR}\"}} 5"
    )));
    assert!(scrape.contains(&format!("fc_fn_active{{address=\"{ADDR}\"}} 0")));
    assert!(scrape.contains("fc_fn_permits_available{scope=\"host\",address=\"\"} 512"));
    assert!(scrape.contains(&format!(
        "fc_fn_permits_available{{scope=\"function\",address=\"{ADDR}\"}} 5"
    )));
}

async fn wait_until(mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the condition never held");
}
