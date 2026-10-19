//! The mandatory time window of the dispatch-job list-shaped reads (Go
//! `ad37de21`, `dispatchjob/api/window_guard_pg_test.go`): the list, list-raw,
//! filter-options and the requeue-by-filter preview and run are held to a
//! `created_at` window. Requires a Postgres (see `support::db`).

use crate::support;

use axum::http::HeaderMap;
use axum::http::StatusCode;
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde_json::{json, Value};
use support::{read_json, TestApp};

/// One read-projection row `age` old (partitions exist from last month, so
/// ages stay under a month), plus its write-table twin for the by-id reads.
async fn seed(app: &TestApp, id: &str, code: &str, status: &str, age: Duration) {
    let created = Utc::now() - age;
    sqlx::query(
        "INSERT INTO msg_dispatch_jobs_read
             (id, code, target_url, kind, protocol, mode, status, max_retries, attempt_count,
              sequence, timeout_seconds, retry_strategy, created_at, updated_at)
         VALUES ($1, $2, 'http://example.invalid/hook', 'EVENT', 'HTTP_WEBHOOK', 'IMMEDIATE', $3, 3, 1,
                 99, 30, 'exponential', $4, $4)",
    )
    .bind(id)
    .bind(code)
    .bind(status)
    .bind(created)
    .execute(&app.pool)
    .await
    .expect("seed read row");
    sqlx::query(
        "INSERT INTO msg_dispatch_jobs (id, code, source, target_url, status, attempt_count, created_at, updated_at)
         VALUES ($1, $2, 'win-test', 'http://example.invalid/hook', $3, 1, $4, $4)",
    )
    .bind(id)
    .bind(code)
    .bind(status)
    .bind(created)
    .execute(&app.pool)
    .await
    .expect("seed write row");
}

fn ts(t: DateTime<Utc>) -> String {
    // `+` in a query string is a space: use the `Z` form.
    t.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn ids(list: &Value) -> Vec<String> {
    let mut v: Vec<String> = list
        .as_array()
        .unwrap_or_else(|| panic!("not an array: {list}"))
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_string())
        .collect();
    v.sort();
    v
}

async fn get(app: &TestApp, token: &str, path: &str) -> (StatusCode, Value, HeaderMap) {
    let resp = app.get(path, token).await;
    let headers = resp.headers().clone();
    let (status, body) = read_json(resp).await;
    (status, body, headers)
}

fn header(h: &HeaderMap, name: &str) -> String {
    h.get(name)
        .unwrap_or_else(|| panic!("no {name} header in {h:?}"))
        .to_str()
        .unwrap()
        .to_string()
}

async fn setup() -> (TestApp, String) {
    let app = TestApp::setup().await;
    let admin = app.anchor_admin_token_all_applications().await;
    (app, admin)
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn a_list_with_no_bounds_is_the_last_24_hours_and_says_so() {
    let (app, admin) = setup().await;
    let code = "wintest:default:job:x";
    seed(&app, "wdfnew0000001", code, "FAILED", Duration::hours(2)).await;
    seed(&app, "wdfold0000001", code, "FAILED", Duration::days(3)).await;

    // The same list on every route that serves it: the API list, list-raw and
    // its /raw alias, and the /bff mounts.
    for path in [
        "/api/dispatch-jobs",
        "/api/dispatch-jobs/list-raw",
        "/api/dispatch-jobs/raw",
        "/bff/dispatch-jobs",
        "/bff/dispatch-jobs/list-raw",
    ] {
        let (status, body, resp) = get(&app, &admin, &format!("{path}?code={code}")).await;
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        assert_eq!(ids(&body), ["wdfnew0000001"], "{path}");
        assert_eq!(header(&resp, "X-Window-Defaulted"), "true", "{path}");
        let from: DateTime<Utc> = header(&resp, "X-Window-From").parse().unwrap();
        let to: DateTime<Utc> = header(&resp, "X-Window-To").parse().unwrap();
        assert_eq!((to - from).num_hours(), 24, "{path}");
        assert!(Utc::now() - to < Duration::minutes(1), "{path}: to is now");
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn since_until_or_both_move_the_window() {
    let (app, admin) = setup().await;
    let code = "wintest:bounds:job:x";
    seed(&app, "wbdnew0000001", code, "FAILED", Duration::hours(2)).await;
    seed(&app, "wbdmid0000001", code, "FAILED", Duration::days(3)).await;
    seed(&app, "wbdold0000001", code, "FAILED", Duration::days(10)).await;
    let list = |q: String| {
        let (app, admin) = (&app, &admin);
        async move {
            let (status, body, resp) =
                get(app, admin, &format!("/api/dispatch-jobs?code={code}&{q}")).await;
            (status, body, resp)
        }
    };

    // since only: since .. now, not defaulted.
    let (status, body, resp) = list(format!("since={}", ts(Utc::now() - Duration::days(5)))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids(&body), ["wbdmid0000001", "wbdnew0000001"]);
    assert_eq!(header(&resp, "X-Window-Defaulted"), "false");

    // until only: 24 hours back from it, defaulted.
    let until = Utc::now() - Duration::days(3) + Duration::hours(1);
    let (status, body, resp) = list(format!("until={}", ts(until))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids(&body), ["wbdmid0000001"]);
    assert_eq!(header(&resp, "X-Window-Defaulted"), "true");
    assert_eq!(header(&resp, "X-Window-To"), ts(until));

    // both: as given.
    let (status, body, resp) = list(format!(
        "since={}&until={}",
        ts(Utc::now() - Duration::days(12)),
        ts(Utc::now() - Duration::days(1))
    ))
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids(&body), ["wbdmid0000001", "wbdold0000001"]);
    assert_eq!(header(&resp, "X-Window-Defaulted"), "false");

    // An unparseable bound on the list is ignored: the default window applies.
    let (status, body, resp) = list("since=yesterday&until=".to_string()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids(&body), ["wbdnew0000001"]);
    assert_eq!(header(&resp, "X-Window-Defaulted"), "true");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn an_inverted_or_too_wide_window_is_a_400_on_every_windowed_read() {
    let (app, admin) = setup().await;
    let now = Utc::now();
    let inverted = format!("since={}&until={}", ts(now), ts(now - Duration::hours(2)));
    let too_wide = format!("since={}", ts(now - Duration::days(32)));
    let exactly_max = format!(
        "since={}",
        ts(now - Duration::days(31) + Duration::minutes(1))
    );

    for path in [
        "/api/dispatch-jobs",
        "/api/dispatch-jobs/list-raw",
        "/api/dispatch-jobs/raw",
        "/api/dispatch-jobs/filter-options",
        "/bff/dispatch-jobs",
        "/bff/dispatch-jobs/filter-options",
    ] {
        let (status, body, _) = get(&app, &admin, &format!("{path}?{inverted}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}: {body}");
        assert_eq!(body["error"], "INVALID_WINDOW", "{path}: {body}");
        let (status, body, _) = get(&app, &admin, &format!("{path}?{too_wide}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}: {body}");
        assert_eq!(body["error"], "WINDOW_TOO_LARGE", "{path}: {body}");
        assert!(body["message"]
            .as_str()
            .unwrap()
            .contains("at most 31 days"));
        let (status, body, _) = get(&app, &admin, &format!("{path}?{exactly_max}")).await;
        assert_eq!(status, StatusCode::OK, "{path} at the maximum: {body}");
    }

    // The POSTs take the bounds in the body.
    for path in [
        "/api/dispatch-jobs/requeue-by-filter/preview",
        "/api/dispatch-jobs/requeue-by-filter",
        "/bff/dispatch-jobs/requeue-by-filter/preview",
    ] {
        let wide = json!({ "since": ts(now - Duration::days(32)), "confirmCount": 0 });
        let (status, body) = read_json(app.post(path, &admin, wide).await).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}: {body}");
        assert_eq!(body["error"], "WINDOW_TOO_LARGE", "{path}: {body}");
        let inv =
            json!({ "since": ts(now), "until": ts(now - Duration::hours(1)), "confirmCount": 0 });
        let (status, body) = read_json(app.post(path, &admin, inv).await).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}: {body}");
        assert_eq!(body["error"], "INVALID_WINDOW", "{path}: {body}");
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn filter_options_and_the_posts_refuse_an_unparseable_bound() {
    let (app, admin) = setup().await;
    let (status, body, _) = get(&app, &admin, "/api/dispatch-jobs/filter-options?since=soon").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "INVALID_TIMESTAMP");
    assert_eq!(body["message"], "since must be an RFC3339 timestamp");
    for path in [
        "/api/dispatch-jobs/requeue-by-filter/preview",
        "/api/dispatch-jobs/requeue-by-filter",
    ] {
        let (status, body) = read_json(
            app.post(path, &admin, json!({ "until": "nope", "confirmCount": 0 }))
                .await,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}: {body}");
        assert_eq!(body["error"], "INVALID_TIMESTAMP", "{path}: {body}");
    }
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn filter_options_read_the_window_and_echo_it() {
    let (app, admin) = setup().await;
    seed(
        &app,
        "wfonew0000001",
        "wintest:fo:new:x",
        "FAILED",
        Duration::hours(2),
    )
    .await;
    seed(
        &app,
        "wfoold0000001",
        "wintest:fo:old:x",
        "EXPIRED",
        Duration::days(3),
    )
    .await;

    let (status, body, _) = get(&app, &admin, "/api/dispatch-jobs/filter-options").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let codes: Vec<&str> = body["codes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(codes.contains(&"wintest:fo:new:x"), "{codes:?}");
    assert!(!codes.contains(&"wintest:fo:old:x"), "{codes:?}");
    assert_eq!(body["window"]["defaulted"], true);

    let since = ts(Utc::now() - Duration::days(5));
    let (status, body, _) = get(
        &app,
        &admin,
        &format!("/bff/dispatch-jobs/filter-options?since={since}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let codes: Vec<&str> = body["codes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(codes.contains(&"wintest:fo:old:x"), "{codes:?}");
    assert_eq!(body["window"]["defaulted"], false);
    assert_eq!(body["window"]["from"], since);
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn the_bulk_requeue_reads_the_window_and_echoes_it() {
    let (app, admin) = setup().await;
    let code = "wintest:bulk:job:x";
    seed(&app, "wblnew0000001", code, "FAILED", Duration::hours(2)).await;
    seed(&app, "wblold0000001", code, "FAILED", Duration::days(3)).await;

    let (status, body) = read_json(
        app.post(
            "/api/dispatch-jobs/requeue-by-filter/preview",
            &admin,
            json!({ "code": code }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["count"], 1,
        "the 3-day-old job is outside the default window: {body}"
    );
    assert_eq!(body["window"]["defaulted"], true);

    let (status, body) = read_json(
        app.post(
            "/api/dispatch-jobs/requeue-by-filter/preview",
            &admin,
            json!({ "code": code, "since": ts(Utc::now() - Duration::days(5)) }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["count"], 2);
    assert_eq!(body["window"]["defaulted"], false);

    let (status, body) = read_json(
        app.post(
            "/api/dispatch-jobs/requeue-by-filter",
            &admin,
            json!({ "code": code, "confirmCount": 1 }),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["requeued"], 1);
    assert_eq!(body["window"]["defaulted"], true);
    let old: String =
        sqlx::query_scalar("SELECT status FROM msg_dispatch_jobs WHERE id = 'wblold0000001'")
            .fetch_one(&app.pool)
            .await
            .unwrap();
    assert_eq!(old, "FAILED", "outside the window: untouched");
}

#[tokio::test]
#[ignore = "requires Docker"]
async fn keyed_lookups_are_not_windowed() {
    let (app, admin) = setup().await;
    let code = "wintest:keyed:job:x";
    seed(&app, "wkyold0000001", code, "FAILED", Duration::days(20)).await;
    let (status, body, resp) = get(&app, &admin, "/api/dispatch-jobs/wkyold0000001").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(resp.get("X-Window-From").is_none());
    let (status, body, _) = get(&app, &admin, "/api/dispatch-jobs/wkyold0000001/raw").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body, _) = get(&app, &admin, "/api/dispatch-jobs/wkyold0000001/attempts").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, _, _) = get(&app, &admin, "/api/dispatch-jobs/by-event/evt_nothere").await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = get(&app, &admin, "/api/dispatch-jobs/event/evt_nothere").await;
    assert_eq!(status, StatusCode::OK);
}
