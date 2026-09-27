//! Function database access (owner decision #7; Java W4, `fc_db_*`,
//! `docs/spec/function-wasm-db.md` in the Java repository): a function
//! reaches the PostgreSQL databases its manifest declares under `db[]`
//! through the host's shared pools, with Java's author contract.
//!
//! | Piece | Where |
//! |---|---|
//! | the connection a `db[].secretRef` names, secret-manager references | [`dsn`] |
//! | one shared bounded pool per connection, the per-function share, the reset on release, secret refresh | [`pools`] |
//! | one invocation's databases and transactions, the deadline, the caps | [`session`] |
//! | `?` placeholders | [`placeholders`] |
//! | parameters bound as their placeholder's type | [`params`] |
//! | rows as JSON, under 10 000 rows / 8 MiB | [`rows`] |
//! | Java's error codes | [`error`] |
//!
//! Runtime-agnostic: the WASM runtime's `flowcatalyst:function/db`
//! interface is a thin layer over [`session`] (`wasm/guest.rs`).

pub mod dsn;
pub mod error;
pub mod numeric;
pub mod params;
pub mod placeholders;
pub mod pools;
pub mod rows;
pub mod session;

pub use dsn::{default_resolver, DsnSource, NoResolver, SecretResolver};
pub use error::{DbErrorCode, DbFailure};
pub use params::Param;
pub use pools::{DbPools, DbSettings, JoinFailure, PoolLease};
pub use rows::RowsAnswer;
pub use session::{Database, DbBindings, DbSession, Transaction};
