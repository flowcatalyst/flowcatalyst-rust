//! `alloc`: allocates and touches `?mb=N` MiB (default 1) of linear memory.

use common::{json, Json, Request};
use wasip2::http::types::{IncomingRequest, ResponseOutparam};

struct Alloc;
wasip2::http::proxy::export!(Alloc);

impl wasip2::exports::http::incoming_handler::Guest for Alloc {
    fn handle(req: IncomingRequest, out: ResponseOutparam) {
        let req = Request::read(req);
        let mb: usize = req.q("mb").and_then(|v| v.parse().ok()).unwrap_or(1);
        let v = vec![7u8; mb * 1024 * 1024];
        let sum: u64 = std::hint::black_box(&v)
            .iter()
            .step_by(4096)
            .map(|b| *b as u64)
            .sum();
        json(
            out,
            200,
            &Json::obj([("allocatedMb", Json::num(mb)), ("sum", Json::num(sum))]),
        );
    }
}
