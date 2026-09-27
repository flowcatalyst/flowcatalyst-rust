//! `fc-server` in Go's router role, started with the production router task
//! definition's environment (`inhance/iac/compute/fc-router.ts`): no
//! database, two config sources merged, the platform token sent to the
//! platform's origin only, pools and queues up, the router surface under
//! `/router` and `/health` at the root. See `support/prod_env.rs`.

mod support;

use std::path::Path;
use std::time::Duration;

use support::prod_env::{
    assert_production_contract, eventually, get, spawn_router, wait_exit, StandIns,
};

fn fc_server() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_fc-server"))
}

#[tokio::test(flavor = "multi_thread")]
async fn router_role_honours_the_production_task_definition() {
    let stand_ins = StandIns::start().await;
    let router = spawn_router(fc_server(), &stand_ins, stand_ins.production_env());

    assert_production_contract(&stand_ins, &router, "/router").await;

    // Go's metrics listener: its own /health.
    let metrics = get(&format!("http://127.0.0.1:{}/health", router.metrics_port))
        .await
        .expect("metrics listener answers");
    assert_eq!(metrics.0, 200);
    // No platform API in this role.
    let me = get(&router.api("/api/me"))
        .await
        .expect("API listener answers");
    assert_eq!(me.0, 404, "the platform is off: {}", me.1);

    let log = router.log();
    assert!(
        log.contains("skipping postgres connect/migrate/seed"),
        "a router-only instance connects to no database"
    );
    assert!(
        !log.contains("router-test-secret"),
        "the client secret is never logged"
    );

    // Decision #43: the task's AUTH_MODE=NONE is honoured for now, loudly:
    // at startup and on the router's health and monitoring output.
    let warning =
        "router API unauthenticated; remove AUTH_MODE=NONE once SDKs send the platform bearer";
    assert!(log.contains(warning), "startup WARN: {log}");
    let health = get(&router.api("/router/health")).await.unwrap();
    assert!(health.1.contains(warning), "{}", health.1);
    let monitoring = get(&router.api("/router/monitoring/health")).await.unwrap();
    assert_eq!(monitoring.0, 200);
    assert!(monitoring.1.contains(warning), "{}", monitoring.1);
    // The mock, test, benchmark and seed routes are dev-only: absent here.
    for path in ["/router/api/test/stats", "/router/api/benchmark/stats"] {
        let answer = get(&router.api(path)).await.unwrap();
        assert_eq!(answer.0, 404, "{path}: {}", answer.1);
    }
}

/// Owner ruling 2: without `AUTH_MODE=NONE` the router's API needs a
/// platform bearer token. The stand-in platform has no discovery document,
/// so every token is refused (fail closed) while delivery, health and the
/// dashboard page carry on.
#[tokio::test(flavor = "multi_thread")]
async fn router_role_without_auth_mode_none_requires_the_platform_bearer() {
    let stand_ins = StandIns::start().await;
    let mut env = stand_ins.production_env();
    env.remove("AUTH_MODE");
    let router = spawn_router(fc_server(), &stand_ins, env);

    eventually(
        Duration::from_secs(30),
        "GET /health answers 200",
        || async {
            get(&router.api("/health"))
                .await
                .filter(|(status, _)| *status == 200)
        },
    )
    .await;
    let pools = get(&router.api("/router/monitoring/pools")).await.unwrap();
    assert_eq!(pools.0, 401, "{}", pools.1);
    assert!(pools.1.contains("UNAUTHORIZED"), "{}", pools.1);
    let resp = reqwest::Client::new()
        .get(router.api("/router/monitoring/pools"))
        .bearer_auth("not-a-platform-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 401);
    assert_eq!(resp.headers()["x-auth-mode"], "BEARER");
    let publish = reqwest::Client::new()
        .post(router.api("/router/messages"))
        .json(&serde_json::json!({"payload": {}, "mediationTarget": "http://127.0.0.1:9/"}))
        .send()
        .await
        .unwrap();
    assert_eq!(publish.status().as_u16(), 401);

    for path in [
        "/router/health",
        "/router/metrics",
        "/router/dashboard.html",
    ] {
        let answer = get(&router.api(path)).await.unwrap();
        assert_ne!(answer.0, 401, "{path} stays open");
    }
    let health = get(&router.api("/router/health")).await.unwrap();
    assert!(!health.1.contains("authWarning"), "{}", health.1);
    let dev = get(&router.api("/router/api/test/stats")).await.unwrap();
    assert_eq!(dev.0, 404);
    let log = router.log();
    assert!(
        log.contains("platform bearer tokens"),
        "startup names the guard: {log}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn router_role_refuses_half_a_platform_credential() {
    let stand_ins = StandIns::start().await;
    let mut env = stand_ins.production_env();
    env.remove("FC_ROUTER_CLIENT_SECRET");
    let router = spawn_router(fc_server(), &stand_ins, env);
    let (status, log) = wait_exit(router, Duration::from_secs(20)).await;
    assert!(!status.success());
    assert!(
        log.contains("FC_ROUTER_CLIENT_ID and FC_ROUTER_CLIENT_SECRET must be set together"),
        "{log}"
    );
    assert!(
        stand_ins.platform_rec.all().is_empty() && stand_ins.integral_rec.all().is_empty(),
        "refused before any config source is contacted"
    );
}
