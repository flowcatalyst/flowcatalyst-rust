//! The standalone `fc-router` binary (`Dockerfile.router`) accepts the same
//! production router environment as `fc-server`'s router role: the same
//! runtime, so the same contract — two config sources merged, the platform
//! token to the platform's origin only, pools and queues up. Its HTTP
//! surface is at the root. See `bin/fc-server/tests/support/prod_env.rs`.

#[path = "../../fc-server/tests/support/prod_env.rs"]
mod prod_env;

use std::path::Path;

use prod_env::{assert_production_contract, spawn_router, StandIns};

#[tokio::test(flavor = "multi_thread")]
async fn standalone_router_honours_the_production_task_definition() {
    let stand_ins = StandIns::start().await;
    let router = spawn_router(
        Path::new(env!("CARGO_BIN_EXE_fc-router-bin")),
        &stand_ins,
        stand_ins.production_env(),
    );
    assert_production_contract(&stand_ins, &router, "").await;
}
