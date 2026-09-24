//! `config`: `?key=K` answers `{"key":K,"value":…}` from `config.get`.

use common::{json, Json, Request};
use wasip2::http::types::{IncomingRequest, ResponseOutparam};

struct Config;
wasip2::http::proxy::export!(Config);

impl wasip2::exports::http::incoming_handler::Guest for Config {
    fn handle(req: IncomingRequest, out: ResponseOutparam) {
        let req = Request::read(req);
        let key = req.q("key").unwrap_or_default();
        let value = fc::config::get(&key);
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
