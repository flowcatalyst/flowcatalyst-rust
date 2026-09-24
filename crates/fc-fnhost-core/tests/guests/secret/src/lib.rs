//! `secret`: `?key=K` answers `{"key":K,"value":…}` from `secrets.get`.

use common::{json, Json, Request};
use wasip2::http::types::{IncomingRequest, ResponseOutparam};

struct Secret;
wasip2::http::proxy::export!(Secret);

impl wasip2::exports::http::incoming_handler::Guest for Secret {
    fn handle(req: IncomingRequest, out: ResponseOutparam) {
        let req = Request::read(req);
        let key = req.q("key").unwrap_or_default();
        let value = fc::secrets::get(&key);
        json(
            out,
            200,
            &Json::obj([
                ("key", Json::str(&key)),
                ("value", Json::opt(value.as_deref())),
            ]),
        );
    }
}
