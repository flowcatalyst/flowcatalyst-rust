//! `fail`: panics (a trap: the guests build with `panic = "abort"`) with a
//! message the caller must never see. `?fail=false` answers normally.
//! `?respondFirst=true` sets the response head before trapping.

use common::{json, Json, Request};
use wasip2::http::types::{IncomingRequest, ResponseOutparam};

struct Fail;
wasip2::http::proxy::export!(Fail);

impl wasip2::exports::http::incoming_handler::Guest for Fail {
    fn handle(req: IncomingRequest, out: ResponseOutparam) {
        let req = Request::read(req);
        if req.q("fail").as_deref() == Some("false") {
            return json(out, 200, &Json::obj([("failed", Json::bool(false))]));
        }
        panic!("the guest failed on purpose: do-not-leak-this");
    }
}
