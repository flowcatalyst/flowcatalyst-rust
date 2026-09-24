//! `spin`: never returns, burning CPU. `?spin=false` answers at once.

use common::{json, Json, Request};
use wasip2::http::types::{IncomingRequest, ResponseOutparam};

struct Spin;
wasip2::http::proxy::export!(Spin);

impl wasip2::exports::http::incoming_handler::Guest for Spin {
    fn handle(req: IncomingRequest, out: ResponseOutparam) {
        let req = Request::read(req);
        if req.q("spin").as_deref() == Some("false") {
            return json(out, 200, &Json::obj([("spun", Json::bool(false))]));
        }
        let mut n: u64 = 0;
        loop {
            n = std::hint::black_box(n.wrapping_add(1));
        }
    }
}
