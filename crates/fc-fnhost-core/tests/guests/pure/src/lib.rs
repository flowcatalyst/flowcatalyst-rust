//! `pure`: a plain `wasi:http/proxy` component with no FlowCatalyst imports
//! at all, to prove the host runs one unchanged. Echoes the method, path,
//! authority and body.

use common::{json, Json, Request};
use wasip2::http::types::{IncomingRequest, ResponseOutparam};

struct Pure;
wasip2::http::proxy::export!(Pure);

impl wasip2::exports::http::incoming_handler::Guest for Pure {
    fn handle(req: IncomingRequest, out: ResponseOutparam) {
        let req = Request::read(req);
        json(
            out,
            200,
            &Json::obj([
                ("pure", Json::bool(true)),
                ("method", Json::str(&req.method)),
                ("pathWithQuery", Json::str(&req.path_with_query)),
                ("authority", Json::opt(req.authority.as_deref())),
                ("body", Json::str(&String::from_utf8_lossy(&req.body))),
            ]),
        );
    }
}
