//! The public listener (Java `FnHttpServerPublicListenerTest`,
//! `FnHostPublicListenerTest`): hostname routing, alias prefixes, trusted
//! proxies, CORS, and the `entry` metrics label.

mod support;

use fc_fnhost_core::host::Listener;
use serde_json::json;
use support::listener::{doc, doc_with_routes, entry, raw, with, Harness, Options};

const ADDR: &str = "app.hello.greet";
const HOST: &str = "api.acme.com";

fn echo(endpoints: serde_json::Value) -> serde_json::Value {
    entry(ADDR, 1, "live", "echo", 10, endpoints)
}

fn routed(entry: serde_json::Value, prefix: &str) -> serde_json::Value {
    doc_with_routes(
        vec![entry],
        json!([{"hostname": HOST, "pathPrefix": prefix, "address": ADDR}]),
    )
}

#[tokio::test]
async fn the_same_function_path_from_the_public_and_private_entries() {
    let h = Harness::start(routed(
        echo(json!([{"path": "/x/{id}", "auth": "none"}])),
        "/billing",
    ))
    .await;
    let public = h.public("GET", HOST, "/billing/x/7?q=a+b", &[]).await;
    assert_eq!(public.status, 200, "{}", public.text());
    let private = h.get(&format!("/functions/{ADDR}/x/7?q=a+b"), &[]).await;
    for key in ["path", "pathParams", "query"] {
        assert_eq!(public.json()[key], private.json()[key], "{key}");
    }
    assert_eq!(public.json()["path"], "/x/7");
    assert_eq!(public.json()["originalPath"], "/billing/x/7");
    assert_eq!(public.json()["originalHost"], HOST);
}

#[tokio::test]
async fn prefix_matching_is_whole_segment_and_an_exact_match_is_the_root() {
    let h = Harness::start(routed(
        echo(json!([{"path": "/*", "auth": "none"}])),
        "/billing",
    ))
    .await;
    let exact = h.public("GET", HOST, "/billing", &[]).await;
    assert_eq!(exact.json()["path"], "/");
    let not_a_segment = h.public("GET", HOST, "/billingx", &[]).await;
    assert_eq!(not_a_segment.status, 404);
    assert_eq!(
        not_a_segment.text(),
        r#"{"error":"NOT_FOUND","message":"not found"}"#
    );
    let with_port = h
        .public("GET", "API.Acme.com:8081", "/billing/z", &[])
        .await;
    assert_eq!(with_port.status, 200, "lower-cased, port stripped");
    assert_eq!(
        h.public("GET", "other.acme.com", "/billing", &[])
            .await
            .status,
        404
    );
    assert_eq!(
        h.public("GET", "127.0.0.1", "/billing", &[]).await.status,
        404
    );
}

#[tokio::test]
async fn x_forwarded_host_is_ignored_and_there_is_no_by_address_access() {
    let h = Harness::start(routed(echo(json!([{"path": "/*", "auth": "none"}])), "/")).await;
    let ignored = h
        .public("GET", "other.acme.com", "/x", &[("X-Forwarded-Host", HOST)])
        .await;
    assert_eq!(ignored.status, 404);
    // the root route owns every path, so `/functions/…` is just a path here
    let by_address = h
        .public("GET", HOST, &format!("/functions/{ADDR}/x"), &[])
        .await;
    assert_eq!(by_address.json()["path"], format!("/functions/{ADDR}/x"));

    let h2 = Harness::start(routed(
        echo(json!([{"path": "/*", "auth": "none"}])),
        "/api",
    ))
    .await;
    for path in [
        format!("/functions/{ADDR}/x"),
        format!("/functions/{ADDR}:1/x"),
    ] {
        let resp = h2.public("GET", HOST, &path, &[]).await;
        assert_eq!(resp.status, 404, "{path}");
        assert_eq!(resp.error(), "NOT_FOUND");
    }
    assert_eq!(h2.probes().total_invocations(), 0);
}

#[tokio::test]
async fn the_inbound_routing_header_never_reaches_the_function() {
    let h = Harness::start(routed(echo(json!([{"path": "/*", "auth": "none"}])), "/")).await;
    let resp = h
        .public(
            "GET",
            HOST,
            "/x",
            &[("X-FlowCatalyst-Function", "evil.app.fn"), ("X-Kept", "1")],
        )
        .await;
    assert!(resp.json()["headers"]
        .get("x-flowcatalyst-function")
        .is_none());
    assert_eq!(resp.json()["headers"]["x-kept"], json!(["1"]));
}

#[tokio::test]
async fn forwarded_for_is_trusted_only_from_a_trusted_proxy() {
    let h = Harness::start(routed(echo(json!([{"path": "/*", "auth": "none"}])), "/")).await;
    // the test connects from loopback, which the default list trusts
    let trusted = h
        .public(
            "GET",
            HOST,
            "/x",
            &[("X-Forwarded-For", "203.0.113.9, 198.51.100.7")],
        )
        .await;
    assert_eq!(
        trusted.json()["remoteAddress"],
        "198.51.100.7",
        "the right-most entry"
    );
    let malformed = h
        .public(
            "GET",
            HOST,
            "/x",
            &[("X-Forwarded-For", "198.51.100.7, not-an-ip")],
        )
        .await;
    assert_eq!(malformed.json()["remoteAddress"], "127.0.0.1");
    let absent = h.public("GET", HOST, "/x", &[]).await;
    assert_eq!(absent.json()["remoteAddress"], "127.0.0.1");
    // the private listener never reads the header
    let private = h
        .get(
            &format!("/functions/{ADDR}/x"),
            &[("X-Forwarded-For", "198.51.100.7")],
        )
        .await;
    assert_eq!(private.json()["remoteAddress"], "127.0.0.1");
}

// ── CORS ─────────────────────────────────────────────────────────────────

fn cors_harness(cors: serde_json::Value, auth: &str, entrypoint: &str) -> serde_json::Value {
    routed(
        entry(
            ADDR,
            1,
            "live",
            entrypoint,
            10,
            json!([{"path": "/api/*", "auth": auth, "methods": ["GET", "POST"], "cors": cors}]),
        ),
        "/",
    )
}

#[tokio::test]
async fn an_allowed_preflight_gets_the_exact_header_set_and_never_invokes() {
    let h = Harness::start(cors_harness(
        json!({"origins": ["https://app.acme.com"], "methods": ["GET", "POST"], "headers": ["X-Custom"]}),
        "none",
        "echo",
    ))
    .await;
    let resp = h
        .public(
            "OPTIONS",
            HOST,
            "/api/x",
            &[
                ("Origin", "https://app.acme.com"),
                ("Access-Control-Request-Method", "POST"),
                ("Access-Control-Request-Headers", "X-Custom"),
            ],
        )
        .await;
    assert_eq!(resp.status, 204);
    assert_eq!(
        resp.headers_all("access-control-allow-origin"),
        ["https://app.acme.com"]
    );
    assert!(resp
        .header("access-control-allow-methods")
        .unwrap()
        .contains("POST"));
    assert_eq!(
        resp.headers_all("access-control-allow-headers"),
        ["X-Custom"]
    );
    assert_eq!(resp.headers_all("access-control-max-age"), ["600"]);
    assert_eq!(resp.headers_all("vary"), ["Origin"]);
    assert!(resp.header("access-control-allow-credentials").is_none());
    assert_eq!(h.probes().total_invocations(), 0);
    let scrape = h.scrape();
    assert!(
        scrape.contains(&format!(
            "fc_fn_invocations_total{{address=\"{ADDR}\",version=\"-\",outcome=\"preflight\",entry=\"public\"}} 1"
        )),
        "{scrape}"
    );
    assert!(!scrape.contains("outcome=\"ok\""));
}

#[tokio::test]
async fn a_disallowed_origin_or_method_gets_no_cors_headers() {
    let h = Harness::start(cors_harness(
        json!({"origins": ["https://app.acme.com"], "methods": ["GET"]}),
        "none",
        "echo",
    ))
    .await;
    for (origin, method) in [
        ("https://evil.example.com", "GET"),
        ("https://app.acme.com", "DELETE"),
    ] {
        let resp = h
            .public(
                "OPTIONS",
                HOST,
                "/api/x",
                &[
                    ("Origin", origin),
                    ("Access-Control-Request-Method", method),
                ],
            )
            .await;
        assert_eq!(resp.status, 204);
        assert!(
            !resp
                .headers
                .keys()
                .any(|k| k.as_str().starts_with("access-control")),
            "{origin} {method}: {:?}",
            resp.headers
        );
        assert!(resp.header("vary").is_none());
    }
}

#[tokio::test]
async fn a_wildcard_origin_gets_star_without_vary() {
    let h = Harness::start(cors_harness(
        json!({"origins": ["*"], "methods": ["GET"]}),
        "none",
        "echo",
    ))
    .await;
    let resp = h
        .public(
            "OPTIONS",
            HOST,
            "/api/x",
            &[
                ("Origin", "https://anyone.example.com"),
                ("Access-Control-Request-Method", "GET"),
            ],
        )
        .await;
    assert_eq!(resp.headers_all("access-control-allow-origin"), ["*"]);
    assert!(resp.header("vary").is_none());
}

#[tokio::test]
async fn the_actual_response_replaces_the_functions_cors_headers() {
    let h = Harness::start(cors_harness(
        json!({"origins": ["https://app.acme.com"], "methods": ["GET"], "allowCredentials": true}),
        "none",
        "cors-setter",
    ))
    .await;
    let allowed = h
        .public("GET", HOST, "/api/x", &[("Origin", "https://app.acme.com")])
        .await;
    assert_eq!(allowed.status, 200);
    assert_eq!(
        allowed.headers_all("access-control-allow-origin"),
        ["https://app.acme.com"]
    );
    assert_eq!(
        allowed.headers_all("access-control-allow-credentials"),
        ["true"]
    );
    assert_eq!(allowed.headers_all("vary"), ["Origin"]);

    let disallowed = h
        .public(
            "GET",
            HOST,
            "/api/x",
            &[("Origin", "https://evil.example.com")],
        )
        .await;
    assert_eq!(disallowed.status, 200, "CORS is not access control");
    assert!(disallowed.header("access-control-allow-origin").is_none());
    assert_eq!(h.probes().total_invocations(), 2);

    // the private listener applies the same policy
    let private = h
        .get(
            &format!("/functions/{ADDR}/api/x"),
            &[("Origin", "https://app.acme.com")],
        )
        .await;
    assert_eq!(
        private.headers_all("access-control-allow-origin"),
        ["https://app.acme.com"]
    );
}

#[tokio::test]
async fn a_preflight_on_a_platform_endpoint_needs_no_auth_but_the_401_still_carries_cors() {
    let h = Harness::start(cors_harness(
        json!({"origins": ["https://app.acme.com"], "methods": ["GET"]}),
        "platform",
        "echo",
    ))
    .await;
    let preflight = h
        .public(
            "OPTIONS",
            HOST,
            "/api/x",
            &[
                ("Origin", "https://app.acme.com"),
                ("Access-Control-Request-Method", "GET"),
            ],
        )
        .await;
    assert_eq!(preflight.status, 204);
    assert_eq!(
        preflight.headers_all("access-control-allow-origin"),
        ["https://app.acme.com"]
    );
    let unauthorized = h
        .public("GET", HOST, "/api/x", &[("Origin", "https://app.acme.com")])
        .await;
    assert_eq!(unauthorized.status, 401);
    assert_eq!(
        unauthorized.headers_all("access-control-allow-origin"),
        ["https://app.acme.com"]
    );
    assert_eq!(h.probes().total_invocations(), 0);
}

// ── metrics, aliases, disabled ───────────────────────────────────────────

#[tokio::test]
async fn the_invocations_counter_carries_the_entry_label() {
    let h = Harness::start(routed(echo(json!([{"path": "/*", "auth": "none"}])), "/")).await;
    assert_eq!(
        h.get(&format!("/functions/{ADDR}/x"), &[]).await.status,
        200
    );
    assert_eq!(h.public("GET", HOST, "/x", &[]).await.status, 200);
    assert_eq!(
        h.public("GET", "nobody.acme.com", "/x", &[]).await.status,
        404
    );
    let scrape = h.scrape();
    for entry in ["private", "public"] {
        assert!(
            scrape.contains(&format!(
                "fc_fn_invocations_total{{address=\"{ADDR}\",version=\"1\",outcome=\"ok\",entry=\"{entry}\"}} 1"
            )),
            "{entry}:\n{scrape}"
        );
    }
    assert!(scrape.contains(
        "fc_fn_invocations_total{address=\"-\",version=\"-\",outcome=\"not_found\",entry=\"public\"} 1"
    ));
}

#[tokio::test]
async fn an_alias_prefixed_hostname_serves_the_aliased_version() {
    let host = "hello.localhost";
    let live = entry(
        ADDR,
        1,
        "live",
        "echo",
        10,
        json!([{"path": "/*", "auth": "none"}]),
    );
    let aliased = with(
        entry(
            ADDR,
            2,
            "alias",
            "echo",
            10,
            json!([{"path": "/*", "auth": "none"}]),
        ),
        json!({"aliases": ["qa"]}),
    );
    let candidate = with(
        entry(
            ADDR,
            3,
            "candidate",
            "echo",
            10,
            json!([{"path": "/*", "auth": "none"}]),
        ),
        json!({"aliases": ["staging"]}),
    );
    let h = Harness::start(doc_with_routes(
        vec![live, aliased, candidate],
        json!([{"hostname": host, "pathPrefix": "/", "address": ADDR, "aliasPrefixes": ["qa", "staging"]}]),
    ))
    .await;
    let qa = h.public("GET", &format!("qa-{host}"), "/x", &[]).await;
    assert_eq!(qa.status, 200, "{}", qa.text());
    assert_eq!(qa.json()["label"], format!("{ADDR}@2"));
    assert_eq!(qa.json()["originalHost"], format!("qa-{host}"));
    let exact = h.public("GET", host, "/x", &[]).await;
    assert_eq!(exact.json()["label"], format!("{ADDR}@1"));
    let not_opted_in = h.public("GET", &format!("dev-{host}"), "/x", &[]).await;
    assert_eq!(not_opted_in.status, 404);
    let candidate_alias = h.public("GET", &format!("staging-{host}"), "/x", &[]).await;
    assert_eq!(
        candidate_alias.status, 404,
        "a candidate never serves an alias"
    );
    assert_eq!(
        h.loader.load_count(&format!("{ADDR}@2")),
        1,
        "loaded through the pinned path"
    );
}

#[tokio::test]
async fn the_public_listener_can_be_disabled() {
    let h = Harness::start_with(
        doc(vec![echo(json!([{"path": "/*", "auth": "none"}]))]),
        Options {
            public: false,
            ..Options::default()
        },
    )
    .await;
    assert_eq!(h.listener.public_port(), None);
    assert!(h.public_base.is_none());
}

#[tokio::test]
async fn the_public_listener_drains_too() {
    let h = Harness::start(routed(echo(json!([{"path": "/*", "auth": "none"}])), "/")).await;
    h.listener.drain().await;
    let resp = raw(
        h.listener.public_port().unwrap(),
        "GET",
        "/x",
        &[("Host", HOST)],
        b"",
    )
    .await;
    assert_eq!(resp.status, 503);
    assert_eq!(resp.error(), "DRAINING");
}
