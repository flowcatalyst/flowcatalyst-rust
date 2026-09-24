//! `log`: `?msg=M` logs `guest info: M` / `guest warn: M` through `log`, and
//! `guest stdout: M` / `guest stderr: M` on standard output and error.
//! `&long=N` also writes N bytes of `x` to stdout with no newline.

use common::{json, Json, Request};
use fc::log::{log, Level};
use std::io::Write;
use wasip2::http::types::{IncomingRequest, ResponseOutparam};

struct Log;
wasip2::http::proxy::export!(Log);

impl wasip2::exports::http::incoming_handler::Guest for Log {
    fn handle(req: IncomingRequest, out: ResponseOutparam) {
        let req = Request::read(req);
        let msg = req.q("msg").unwrap_or_else(|| "hello".into());
        log(Level::Info, &format!("guest info: {msg}"));
        log(Level::Warn, &format!("guest warn: {msg}"));
        println!("guest stdout: {msg}");
        eprintln!("guest stderr: {msg}");
        if let Some(n) = req.q("long").and_then(|v| v.parse::<usize>().ok()) {
            let mut stdout = std::io::stdout();
            let _ = stdout.write_all(&vec![b'x'; n]);
            let _ = stdout.flush();
        }
        json(out, 200, &Json::obj([("logged", Json::str(&msg))]));
    }
}
