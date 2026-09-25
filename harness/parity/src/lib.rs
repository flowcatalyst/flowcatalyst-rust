//! The API parity harness, ported from `flowcatalyst-javalin/parity`
//! (owner decision #29): the same scenario files run against Go's and
//! Rust's `fc-server`, each on a byte-identical clone of one Go-created
//! seed database; every response is normalised and diffed, and every
//! difference is either allow-listed with an owner decision or reported.
//! See `README.md` for usage and the design notes.

pub mod authenticator;
pub mod binaries;
pub mod coverage;
pub mod diff;
pub mod expected;
pub mod keys;
pub mod loader;
pub mod model;
pub mod normaliser;
pub mod parity;
pub mod pg;
pub mod record;
pub mod report;
pub mod runner;
pub mod seed;
pub mod side;
pub mod substitution;
pub mod totp;
pub mod vars;

pub use parity::{run, Config};
pub use report::Report;
