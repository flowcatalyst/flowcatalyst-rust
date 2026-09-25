//! `fc-server` in Go's router role, started with the production router task
//! definition's environment (`inhance/iac/compute/fc-router.ts`): no
//! database, two config sources merged, the platform token sent to the
//! platform's origin only, pools and queues up, the router surface under
//! `/router` and `/health` at the root. See `support/prod_env.rs`.

mod support;

use std::path::Path;
use std::time::Duration;

use support::prod_env::{assert_production_contract, get, spawn_router, wait_exit, StandIns};

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
