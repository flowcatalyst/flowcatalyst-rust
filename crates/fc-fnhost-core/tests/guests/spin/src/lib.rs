//! `spin`: never returns, burning CPU. `?spin=false` answers at once.
//! `?n=N` spins N loop iterations and answers (a tight loop: fuel
//! metering's worst case); `?hash=N` hashes a 64 KiB buffer N times and
//! answers (loads, stores and arithmetic: a more typical mix). Both are
//! the fuel-overhead measurement's workloads (`tests/wasm_fuel.rs`).

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
        if let Some(n) = req.q("n").and_then(|v| v.parse::<u64>().ok()) {
            let mut i: u64 = 0;
            while i < n {
                i = std::hint::black_box(i + 1);
            }
            return json(out, 200, &Json::obj([("spun", Json::num(i))]));
        }
        if let Some(rounds) = req.q("hash").and_then(|v| v.parse::<u64>().ok()) {
            let mut buffer = vec![0u8; 64 * 1024];
            let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
            for round in 0..rounds {
                for (i, byte) in buffer.iter_mut().enumerate() {
                    hash ^= (*byte as u64) ^ (i as u64) ^ round;
                    hash = hash.wrapping_mul(0x0100_0000_01b3);
                    *byte = (hash >> 56) as u8;
                }
            }
            let hash = std::hint::black_box(hash);
            return json(out, 200, &Json::obj([("hash", Json::num(hash & 0xffff))]));
        }
        let mut n: u64 = 0;
        loop {
            n = std::hint::black_box(n.wrapping_add(1));
        }
    }
}
