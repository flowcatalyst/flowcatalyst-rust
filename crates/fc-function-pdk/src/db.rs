//! The databases a function's manifest declares under `db[]` (WIT
//! `flowcatalyst:function/db`, 0.1.2; Java's `fc_db_*` author contract).
//!
//! ```ignore
//! use fc_function_pdk::prelude::*;
//! use fc_function_pdk::params;
//!
//! #[handler]
//! fn handle(req: Request, ctx: Context) -> Result<Response, Error> {
//!     let db = ctx.db("orders")?;
//!     let order = req.path_param("id").unwrap_or_default().to_owned();
//!     let tx = db.begin()?;
//!     tx.execute("UPDATE orders SET state = 'shipped' WHERE id = ?", params![order.as_str()])?;
//!     tx.execute("INSERT INTO shipments (order_id) VALUES (?)", params![order.as_str()])?;
//!     tx.commit()?;                   // dropped without commit: rolled back
//!     let rows = db.query("SELECT id, state FROM orders WHERE id = ?", params![order.as_str()])?;
//!     Ok(Response::json(200, rows.json())?)
//! }
//! ```
//!
//! - Parameters are bound to the statement's `?` placeholders, in order;
//!   never interpolated. `??` is a literal `?` (jsonb's `?`, `?|`, `?&`).
//!   An integer, a float, a boolean and a [`Param::Decimal`] are sent typed
//!   (`int8`, `float8`, `bool`, `numeric`); text and `NULL` untyped, so the
//!   server reads text as whatever the placeholder needs (`uuid`,
//!   `timestamptz`, `jsonb`, …).
//! - Outside a [`Transaction`] each statement runs on a connection of its
//!   own, in autocommit. A transaction holds one connection until
//!   [`commit`](Transaction::commit), [`rollback`](Transaction::rollback) or
//!   drop (drop rolls back); the invocation ending rolls back whatever is
//!   still open.
//! - Each statement's timeout is the time left before the invocation's
//!   deadline (`DB_TIMEOUT`).
//! - A query answers at most 10 000 rows or 8 MiB of row JSON
//!   ([`Rows::truncated`]).
//! - Row values follow Java's mapping: numbers, `numeric` as its exact
//!   text, booleans, ISO-8601 times (`timestamptz` in UTC with `Z`), `bytea`
//!   base64, `json`/`jsonb` as JSON, anything else as PostgreSQL's text.
//! - Every failure is a [`DbError`] carrying Java's code.

use std::fmt;

/// A statement parameter (see the module docs for how each is sent).
#[derive(Debug, Clone, PartialEq)]
pub enum Param {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    /// An exact decimal's text (`"12.50"`), sent as `numeric`.
    Decimal(String),
    Text(String),
}

macro_rules! from_int {
    ($($t:ty),*) => {$(
        impl From<$t> for Param {
            fn from(v: $t) -> Self {
                Param::Int(v as i64)
            }
        }
    )*};
}
from_int!(i8, i16, i32, i64, u8, u16, u32);

impl From<f32> for Param {
    fn from(v: f32) -> Self {
        Param::Float(v as f64)
    }
}

impl From<f64> for Param {
    fn from(v: f64) -> Self {
        Param::Float(v)
    }
}

impl From<bool> for Param {
    fn from(v: bool) -> Self {
        Param::Bool(v)
    }
}

impl From<&str> for Param {
    fn from(v: &str) -> Self {
        Param::Text(v.to_owned())
    }
}

impl From<String> for Param {
    fn from(v: String) -> Self {
        Param::Text(v)
    }
}

impl From<&String> for Param {
    fn from(v: &String) -> Self {
        Param::Text(v.clone())
    }
}

impl<T: Into<Param>> From<Option<T>> for Param {
    fn from(v: Option<T>) -> Self {
        v.map_or(Param::Null, Into::into)
    }
}

/// A JSON value as a parameter, as Java binds one: an integer that fits
/// as `int8`, any other number as `numeric` (its exact text), a string as
/// text, `null` as `NULL`, a boolean as `bool`, an array or object as its
/// JSON text (for a `json`/`jsonb` placeholder).
#[cfg(feature = "json")]
impl From<&serde_json::Value> for Param {
    fn from(v: &serde_json::Value) -> Self {
        use serde_json::Value as V;
        match v {
            V::Null => Param::Null,
            V::Bool(b) => Param::Bool(*b),
            V::Number(n) => match n.as_i64() {
                Some(i) => Param::Int(i),
                None => Param::Decimal(n.to_string()),
            },
            V::String(s) => Param::Text(s.clone()),
            other => Param::Text(other.to_string()),
        }
    }
}

#[cfg(feature = "json")]
impl From<serde_json::Value> for Param {
    fn from(v: serde_json::Value) -> Self {
        Param::from(&v)
    }
}

/// `params![a, b, …]`: a `&[Param]` of anything that converts into a
/// [`Param`].
#[macro_export]
macro_rules! params {
    () => {
        &[] as &[$crate::db::Param]
    };
    ($($value:expr),+ $(,)?) => {
        &[$($crate::db::Param::from($value)),+] as &[$crate::db::Param]
    };
}

/// A query's answer: the rows as a JSON array of objects keyed by column
/// label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rows {
    json: String,
    count: u32,
    truncated: bool,
}

impl Rows {
    pub fn new(json: impl Into<String>, count: u32, truncated: bool) -> Self {
        Self {
            json: json.into(),
            count,
            truncated,
        }
    }

    /// The rows array, as JSON text.
    pub fn json(&self) -> &str {
        &self.json
    }

    pub fn len(&self) -> usize {
        self.count as usize
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// There were more rows than the caps (10 000 rows, 8 MiB) let through.
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// The rows as JSON values.
    #[cfg(feature = "json")]
    pub fn values(&self) -> Result<Vec<serde_json::Value>, DbError> {
        self.parse()
    }

    /// The rows deserialized, one `T` per row.
    #[cfg(feature = "json")]
    pub fn parse<T: serde::de::DeserializeOwned>(&self) -> Result<Vec<T>, DbError> {
        serde_json::from_str(&self.json).map_err(|e| DbError {
            code: DbErrorCode::BadRequest,
            message: format!("the rows do not deserialize: {e}"),
        })
    }
}

/// Java's codes for a failed database call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DbErrorCode {
    /// `DB_NOT_DECLARED`: no `db[]` entry by that name.
    NotDeclared,
    /// `DB_BAD_REQUEST`: a parameter the placeholder's type cannot take, a
    /// placeholder count that does not match, or too many connections held
    /// at once.
    BadRequest,
    /// `DB_TX_UNKNOWN`: the transaction is no longer open.
    TxUnknown,
    /// `DB_CONSTRAINT`: SQLSTATE class 23.
    Constraint,
    /// `DB_SYNTAX`: SQLSTATE class 42.
    Syntax,
    /// `DB_TIMEOUT`: past the invocation's deadline.
    Timeout,
    /// `DB_UNAVAILABLE`: the database could not be reached, or is out of
    /// connections. Worth a retry.
    Unavailable,
    /// `DB_ERROR`: anything else the database refused.
    Error,
}

impl DbErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            DbErrorCode::NotDeclared => "DB_NOT_DECLARED",
            DbErrorCode::BadRequest => "DB_BAD_REQUEST",
            DbErrorCode::TxUnknown => "DB_TX_UNKNOWN",
            DbErrorCode::Constraint => "DB_CONSTRAINT",
            DbErrorCode::Syntax => "DB_SYNTAX",
            DbErrorCode::Timeout => "DB_TIMEOUT",
            DbErrorCode::Unavailable => "DB_UNAVAILABLE",
            DbErrorCode::Error => "DB_ERROR",
        }
    }
}

/// A failed database call: Java's code and a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbError {
    pub code: DbErrorCode,
    pub message: String,
}

impl DbError {
    pub fn new(code: DbErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// The wire code: `DB_CONSTRAINT`, `DB_TIMEOUT`, …
    pub fn code(&self) -> &'static str {
        self.code.as_str()
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    /// Whether a retry of the whole call may succeed (`DB_UNAVAILABLE`,
    /// `DB_TIMEOUT`).
    pub fn is_transient(&self) -> bool {
        matches!(self.code, DbErrorCode::Unavailable | DbErrorCode::Timeout)
    }
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for DbError {}

/// Opens a database by name (the test double; the host's is
/// `crate::wasi::open_db`).
pub(crate) trait DbOpener {
    fn open(&self, name: &str) -> Result<Box<dyn DbBackend>, DbError>;
}

/// What a backend (the host, or the test double) gives for one database.
pub(crate) trait DbBackend {
    fn query(&self, sql: &str, params: &[Param]) -> Result<Rows, DbError>;
    fn execute(&self, sql: &str, params: &[Param]) -> Result<u64, DbError>;
    fn begin(&self) -> Result<Box<dyn TxBackend>, DbError>;
}

/// And for one open transaction. Dropping it (without `finish`) rolls back.
pub(crate) trait TxBackend {
    fn query(&self, sql: &str, params: &[Param]) -> Result<Rows, DbError>;
    fn execute(&self, sql: &str, params: &[Param]) -> Result<u64, DbError>;
    fn finish(self: Box<Self>, commit: bool) -> Result<(), DbError>;
}

/// A database the manifest declares, from [`Context::db`](crate::Context::db).
pub struct Db {
    name: String,
    backend: Box<dyn DbBackend>,
}

impl Db {
    pub(crate) fn new(name: String, backend: Box<dyn DbBackend>) -> Self {
        Self { name, backend }
    }

    /// The `db[].name`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Runs a statement that returns rows, on a connection of its own.
    pub fn query(&self, sql: &str, params: &[Param]) -> Result<Rows, DbError> {
        self.backend.query(sql, params)
    }

    /// Runs a statement for its effect, on a connection of its own; the
    /// number of rows it affected.
    pub fn execute(&self, sql: &str, params: &[Param]) -> Result<u64, DbError> {
        self.backend.execute(sql, params)
    }

    /// Opens a transaction on a connection of its own.
    pub fn begin(&self) -> Result<Transaction, DbError> {
        Ok(Transaction {
            backend: Some(self.backend.begin()?),
        })
    }

    /// Runs `work` in a transaction: committed when it returns `Ok`, rolled
    /// back when it returns `Err` (whose error is returned).
    pub fn transaction<T, E: From<DbError>>(
        &self,
        work: impl FnOnce(&Transaction) -> Result<T, E>,
    ) -> Result<T, E> {
        let tx = self.begin()?;
        let value = work(&tx)?;
        tx.commit()?;
        Ok(value)
    }
}

impl fmt::Debug for Db {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Db({})", self.name)
    }
}

/// An open transaction: a guard that rolls back when dropped uncommitted.
pub struct Transaction {
    backend: Option<Box<dyn TxBackend>>,
}

impl Transaction {
    fn backend(&self) -> Result<&dyn TxBackend, DbError> {
        self.backend
            .as_deref()
            .ok_or_else(|| DbError::new(DbErrorCode::TxUnknown, "the transaction is over"))
    }

    pub fn query(&self, sql: &str, params: &[Param]) -> Result<Rows, DbError> {
        self.backend()?.query(sql, params)
    }

    pub fn execute(&self, sql: &str, params: &[Param]) -> Result<u64, DbError> {
        self.backend()?.execute(sql, params)
    }

    /// Commits. Over whatever the outcome: a failed commit rolled back.
    pub fn commit(mut self) -> Result<(), DbError> {
        self.finish(true)
    }

    pub fn rollback(mut self) -> Result<(), DbError> {
        self.finish(false)
    }

    fn finish(&mut self, commit: bool) -> Result<(), DbError> {
        match self.backend.take() {
            Some(backend) => backend.finish(commit),
            None => Err(DbError::new(
                DbErrorCode::TxUnknown,
                "the transaction is over",
            )),
        }
    }
}

impl fmt::Debug for Transaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.backend.is_some() {
            "Transaction(open)"
        } else {
            "Transaction(over)"
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_convert_into_params() {
        let p: &[Param] =
            crate::params![1, "a", 2.5, true, None::<i32>, Some("b"), String::from("c")];
        assert_eq!(
            p,
            &[
                Param::Int(1),
                Param::Text("a".into()),
                Param::Float(2.5),
                Param::Bool(true),
                Param::Null,
                Param::Text("b".into()),
                Param::Text("c".into()),
            ]
        );
        assert!(crate::params![].is_empty());
    }

    #[cfg(feature = "json")]
    #[test]
    fn json_values_convert_as_java_binds_them() {
        let v = serde_json::json!([1, 1.5, "x", null, true, {"a": 1}]);
        let params: Vec<Param> = v.as_array().unwrap().iter().map(Param::from).collect();
        assert_eq!(
            params,
            vec![
                Param::Int(1),
                Param::Decimal("1.5".into()),
                Param::Text("x".into()),
                Param::Null,
                Param::Bool(true),
                Param::Text(r#"{"a":1}"#.into()),
            ]
        );
    }

    #[test]
    fn errors_carry_javas_codes() {
        let e = DbError::new(DbErrorCode::Constraint, "duplicate key");
        assert_eq!(e.code(), "DB_CONSTRAINT");
        assert_eq!(e.to_string(), "DB_CONSTRAINT: duplicate key");
        assert!(!e.is_transient());
        assert!(DbError::new(DbErrorCode::Unavailable, "").is_transient());
    }
}
