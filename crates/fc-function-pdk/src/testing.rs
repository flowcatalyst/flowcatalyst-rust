//! A stand-in for the FlowCatalyst host, so a function's unit tests run
//! natively with `cargo test`.
//!
//! ```
//! # #[cfg(feature = "flowcatalyst")] {
//! use fc_function_pdk::prelude::*;
//! use fc_function_pdk::testing::TestHost;
//!
//! async fn handle(req: Request, ctx: Context) -> Result<Response, Error> {
//!     let greeting = ctx.config().require("GREETING")?;
//!     Ok(Response::json(200, format!("{{\"say\":\"{greeting}, {}\"}}", req.path_param("name").unwrap_or("?")))?)
//! }
//!
//! let host = TestHost::new().config("GREETING", "hello").path_param("name", "ada");
//! let response = fc_function_pdk::block_on(handle(host.request("GET", "/hi/ada"), host.context())).unwrap();
//! assert_eq!(response.body(), br#"{"say":"hello, ada"}"#);
//! # }
//! ```

use std::cell::RefCell;
#[cfg(feature = "flowcatalyst")]
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use fc_function_abi::MultiMap;
#[cfg(feature = "flowcatalyst")]
use fc_function_abi::{emit_error, Caller, FunctionAddress, OutboundEvent};

use crate::backend::{Backend, HttpFuture};
use crate::context::{Context, Level};
#[cfg(feature = "flowcatalyst")]
use crate::context::{EmitError, Invocation};
use crate::http::{target, HttpCall, HttpDenied, HttpError, HttpReply};
use crate::request::Request;

type HttpResponder = Box<dyn Fn(&HttpCall) -> Result<HttpReply, HttpError>>;
#[cfg(feature = "flowcatalyst")]
type EmitResponder = Box<dyn Fn(&OutboundEvent) -> Result<(), EmitError>>;

/// A fake host: config, secrets, the invocation, an outbound HTTP responder,
/// an emit responder and a fixed clock, plus a record of what the function
/// did (events emitted, calls made, lines logged).
///
/// Configure it first (the builder methods), then take
/// [`request`](Self::request)s and [`context`](Self::context)s from it.
pub struct TestHost {
    backend: Rc<TestBackend>,
}

struct TestBackend {
    #[cfg(feature = "flowcatalyst")]
    invocation: Invocation,
    #[cfg(feature = "flowcatalyst")]
    config: HashMap<String, String>,
    #[cfg(feature = "flowcatalyst")]
    secrets: HashMap<String, String>,
    #[cfg(feature = "flowcatalyst")]
    emit: EmitResponder,
    #[cfg(feature = "flowcatalyst")]
    emitted: RefCell<Vec<OutboundEvent>>,
    #[cfg(feature = "flowcatalyst")]
    databases: Rc<TestDatabases>,
    http: HttpResponder,
    calls: RefCell<Vec<HttpCall>>,
    logs: RefCell<Vec<(Level, String)>>,
    now: SystemTime,
}

/// 2026-01-01T00:00:00Z.
const DEFAULT_NOW: Duration = Duration::from_secs(1_767_225_600);

impl Default for TestHost {
    fn default() -> Self {
        Self::new()
    }
}

impl TestHost {
    /// No config, no secrets; the function `example.service.function`
    /// version 1, called anonymously; every outbound call denied (an empty
    /// `httpAllow`); every event accepted; the clock at
    /// 2026-01-01T00:00:00Z.
    pub fn new() -> Self {
        Self {
            backend: Rc::new(TestBackend {
                #[cfg(feature = "flowcatalyst")]
                invocation: Invocation::new(
                    FunctionAddress::parse("example.service.function").expect("a valid address"),
                ),
                #[cfg(feature = "flowcatalyst")]
                config: HashMap::new(),
                #[cfg(feature = "flowcatalyst")]
                secrets: HashMap::new(),
                #[cfg(feature = "flowcatalyst")]
                emit: Box::new(|_| Ok(())),
                #[cfg(feature = "flowcatalyst")]
                emitted: RefCell::new(Vec::new()),
                #[cfg(feature = "flowcatalyst")]
                databases: Rc::new(TestDatabases::default()),
                http: Box::new(|call| {
                    let host = target(call.url())?.host.to_owned();
                    Err(HttpError::Denied(HttpDenied::new(host)))
                }),
                calls: RefCell::new(Vec::new()),
                logs: RefCell::new(Vec::new()),
                now: SystemTime::UNIX_EPOCH + DEFAULT_NOW,
            }),
        }
    }

    fn state(&mut self) -> &mut TestBackend {
        Rc::get_mut(&mut self.backend)
            .expect("configure the TestHost before taking a request or context from it")
    }

    /// A config value the manifest declares.
    #[cfg(feature = "flowcatalyst")]
    pub fn config(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.state().config.insert(key.into(), value.into());
        self
    }

    /// A secret the manifest declares. An empty value reads as absent, as on
    /// the host.
    #[cfg(feature = "flowcatalyst")]
    pub fn secret(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        let value = value.into();
        if !value.is_empty() {
            self.state().secrets.insert(key.into(), value);
        }
        self
    }

    /// Replaces the whole invocation.
    #[cfg(feature = "flowcatalyst")]
    pub fn invocation(mut self, invocation: Invocation) -> Self {
        self.state().invocation = invocation;
        self
    }

    /// Changes the invocation in place.
    #[cfg(feature = "flowcatalyst")]
    pub fn with_invocation(mut self, change: impl FnOnce(&mut Invocation)) -> Self {
        change(&mut self.state().invocation);
        self
    }

    /// The caller.
    #[cfg(feature = "flowcatalyst")]
    pub fn caller(self, caller: Caller) -> Self {
        self.with_invocation(|i| i.caller = caller)
    }

    /// A path parameter the endpoint's pattern bound.
    #[cfg(feature = "flowcatalyst")]
    pub fn path_param(self, name: impl Into<String>, value: impl Into<String>) -> Self {
        let pair = (name.into(), value.into());
        self.with_invocation(|i| i.path_params.push(pair))
    }

    /// How [`Events::emit`](crate::Events::emit) answers, after the host's
    /// own checks (a blank type, data that is not JSON). The default accepts
    /// every event. An accepted event gets the id `evt_<n>`, the n-th event
    /// accepted.
    #[cfg(feature = "flowcatalyst")]
    pub fn emit_with(
        mut self,
        respond: impl Fn(&OutboundEvent) -> Result<(), EmitError> + 'static,
    ) -> Self {
        self.state().emit = Box::new(respond);
        self
    }

    /// A database the manifest declares under `db[]` as `name`, answering
    /// every statement with `respond` (a query wants [`DbReply::Rows`], an
    /// execute [`DbReply::Updated`]). Any other name is `DB_NOT_DECLARED`,
    /// as on the host. What the function did is in
    /// [`db_events`](Self::db_events).
    #[cfg(feature = "flowcatalyst")]
    pub fn db(
        mut self,
        name: impl Into<String>,
        respond: impl Fn(&DbCall<'_>) -> Result<DbReply, crate::db::DbError> + 'static,
    ) -> Self {
        let databases = Rc::get_mut(&mut self.state().databases)
            .expect("configure the TestHost before taking a request or context from it");
        databases.responders.insert(name.into(), Box::new(respond));
        self
    }

    /// Every statement, `begin`, `commit`, `rollback` and dropped
    /// transaction, in order.
    #[cfg(feature = "flowcatalyst")]
    pub fn db_events(&self) -> Vec<DbEvent> {
        self.backend.databases.events.borrow().clone()
    }

    /// How outbound calls answer. The default denies every one, as an empty
    /// `httpAllow` does.
    pub fn http(
        mut self,
        respond: impl Fn(&HttpCall) -> Result<HttpReply, HttpError> + 'static,
    ) -> Self {
        self.state().http = Box::new(respond);
        self
    }

    /// The time [`Context::now`] answers.
    pub fn now(mut self, now: SystemTime) -> Self {
        self.state().now = now;
        self
    }

    /// A context on this host.
    pub fn context(&self) -> Context {
        Context::new(self.backend.clone())
    }

    /// A request on this host, with no headers and no body (add them with
    /// [`Request::with_header`], [`Request::with_body`]).
    pub fn request(&self, method: &str, path_with_query: &str) -> Request {
        Request::new(
            method.to_owned(),
            path_with_query,
            MultiMap::new(),
            Vec::new(),
            Some("fn.test".into()),
            self.backend.clone(),
        )
    }

    /// The events the platform accepted, as it received them: a `None`
    /// correlation or causation id replaced by the invocation's.
    #[cfg(feature = "flowcatalyst")]
    pub fn emitted(&self) -> Vec<OutboundEvent> {
        self.backend.emitted.borrow().clone()
    }

    /// Every outbound call the function made, denied ones included.
    pub fn http_calls(&self) -> Vec<HttpCall> {
        self.backend.calls.borrow().clone()
    }

    /// Every line the function logged through its [`crate::Logger`].
    pub fn logs(&self) -> Vec<(Level, String)> {
        self.backend.logs.borrow().clone()
    }
}

impl Backend for TestBackend {
    #[cfg(feature = "flowcatalyst")]
    fn invocation(&self) -> &Invocation {
        &self.invocation
    }

    #[cfg(feature = "flowcatalyst")]
    fn config(&self, key: &str) -> Option<String> {
        self.config.get(key).cloned()
    }

    #[cfg(feature = "flowcatalyst")]
    fn secret(&self, key: &str) -> Option<String> {
        self.secrets.get(key).cloned()
    }

    /// The host's checks, in its order, then the responder.
    #[cfg(feature = "flowcatalyst")]
    fn emit(&self, event: &OutboundEvent) -> Result<String, EmitError> {
        if event.event_type().trim().is_empty() {
            return Err(EmitError::Invalid(
                emit_error::INVALID_EVENT_TYPE_REQUIRED.into(),
            ));
        }
        if event.dedup_id().trim().is_empty() {
            return Err(EmitError::Invalid(emit_error::DEDUP_ID_REQUIRED.into()));
        }
        if !event.data().is_empty() && !is_json(event.data()) {
            return Err(EmitError::Invalid(
                emit_error::INVALID_EVENT_DATA_NOT_JSON.into(),
            ));
        }
        (self.emit)(event)?;
        let mut received = event.clone();
        if event.correlation_id().is_none() {
            received = received.with_correlation_id(self.invocation.correlation_id.clone());
        }
        if let (None, Some(causation)) = (event.causation_id(), &self.invocation.causation_id) {
            received = received.with_causation_id(causation.clone());
        }
        let mut emitted = self.emitted.borrow_mut();
        emitted.push(received);
        Ok(format!("evt_{}", emitted.len()))
    }

    #[cfg(feature = "flowcatalyst")]
    fn test_databases(&self) -> Option<&dyn crate::db::DbOpener> {
        Some(&self.databases)
    }

    fn log(&self, level: Level, message: &str) {
        self.logs.borrow_mut().push((level, message.to_owned()));
    }

    fn send(&self, call: HttpCall) -> HttpFuture {
        self.calls.borrow_mut().push(call.clone());
        let reply = match target(call.url()) {
            Err(e) => Err(e),
            Ok(_) if call.timeout().is_some_and(|t| t.is_zero()) => Err(HttpError::InvalidRequest(
                "the timeout must be positive".into(),
            )),
            Ok(_) => (self.http)(&call),
        };
        Box::pin(std::future::ready(reply))
    }

    fn now(&self) -> SystemTime {
        self.now
    }
}

// ── test databases ───────────────────────────────────────────────────────

#[cfg(feature = "flowcatalyst")]
type DbResponder = Box<dyn Fn(&DbCall<'_>) -> Result<DbReply, crate::db::DbError>>;

#[cfg(feature = "flowcatalyst")]
#[derive(Default)]
struct TestDatabases {
    responders: HashMap<String, DbResponder>,
    events: RefCell<Vec<DbEvent>>,
    next_transaction: std::cell::Cell<u32>,
}

/// One statement the function ran on a test database.
#[cfg(feature = "flowcatalyst")]
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct DbCall<'a> {
    pub db: &'a str,
    /// The transaction it ran in (`1`, `2`, … in `begin` order).
    pub transaction: Option<u32>,
    pub sql: &'a str,
    pub params: &'a [crate::db::Param],
    /// `query` (rows wanted) rather than `execute`.
    pub query: bool,
}

/// A test database's answer to a statement.
#[cfg(feature = "flowcatalyst")]
#[derive(Debug, Clone, PartialEq)]
pub enum DbReply {
    Rows(crate::db::Rows),
    Updated(u64),
}

#[cfg(all(feature = "flowcatalyst", feature = "json"))]
impl DbReply {
    /// Rows from a JSON array of objects.
    pub fn rows(rows: serde_json::Value) -> Self {
        let count = rows.as_array().map_or(0, Vec::len) as u32;
        DbReply::Rows(crate::db::Rows::new(rows.to_string(), count, false))
    }
}

/// What a function did to a test database.
#[cfg(feature = "flowcatalyst")]
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum DbEvent {
    Statement {
        db: String,
        transaction: Option<u32>,
        sql: String,
        params: Vec<crate::db::Param>,
        query: bool,
    },
    Begin {
        db: String,
        transaction: u32,
    },
    Commit {
        transaction: u32,
    },
    Rollback {
        transaction: u32,
    },
    /// Dropped while open: the host rolls it back.
    Dropped {
        transaction: u32,
    },
}

#[cfg(feature = "flowcatalyst")]
struct TestDb {
    databases: Rc<TestDatabases>,
    name: String,
}

#[cfg(feature = "flowcatalyst")]
struct TestTx {
    databases: Rc<TestDatabases>,
    name: String,
    id: u32,
    over: bool,
}

/// `databases` is an `Rc` so each open database can hold it.
#[cfg(feature = "flowcatalyst")]
impl crate::db::DbOpener for Rc<TestDatabases> {
    fn open(&self, name: &str) -> Result<Box<dyn crate::db::DbBackend>, crate::db::DbError> {
        if !self.responders.contains_key(name) {
            return Err(crate::db::DbError::new(
                crate::db::DbErrorCode::NotDeclared,
                format!("no database named '{name}' is declared by this function's manifest"),
            ));
        }
        Ok(Box::new(TestDb {
            databases: self.clone(),
            name: name.to_owned(),
        }))
    }
}

#[cfg(feature = "flowcatalyst")]
impl TestDatabases {
    fn statement(
        &self,
        db: &str,
        transaction: Option<u32>,
        sql: &str,
        params: &[crate::db::Param],
        query: bool,
    ) -> Result<DbReply, crate::db::DbError> {
        self.events.borrow_mut().push(DbEvent::Statement {
            db: db.to_owned(),
            transaction,
            sql: sql.to_owned(),
            params: params.to_vec(),
            query,
        });
        let respond = &self.responders[db];
        respond(&DbCall {
            db,
            transaction,
            sql,
            params,
            query,
        })
    }
}

#[cfg(feature = "flowcatalyst")]
fn rows_of(reply: DbReply) -> crate::db::Rows {
    match reply {
        DbReply::Rows(rows) => rows,
        DbReply::Updated(_) => crate::db::Rows::new("[]", 0, false),
    }
}

#[cfg(feature = "flowcatalyst")]
fn updated_of(reply: DbReply) -> u64 {
    match reply {
        DbReply::Updated(n) => n,
        DbReply::Rows(rows) => rows.len() as u64,
    }
}

#[cfg(feature = "flowcatalyst")]
impl crate::db::DbBackend for TestDb {
    fn query(
        &self,
        sql: &str,
        params: &[crate::db::Param],
    ) -> Result<crate::db::Rows, crate::db::DbError> {
        self.databases
            .statement(&self.name, None, sql, params, true)
            .map(rows_of)
    }

    fn execute(&self, sql: &str, params: &[crate::db::Param]) -> Result<u64, crate::db::DbError> {
        self.databases
            .statement(&self.name, None, sql, params, false)
            .map(updated_of)
    }

    fn begin(&self) -> Result<Box<dyn crate::db::TxBackend>, crate::db::DbError> {
        let id = self.databases.next_transaction.get() + 1;
        self.databases.next_transaction.set(id);
        self.databases.events.borrow_mut().push(DbEvent::Begin {
            db: self.name.clone(),
            transaction: id,
        });
        Ok(Box::new(TestTx {
            databases: self.databases.clone(),
            name: self.name.clone(),
            id,
            over: false,
        }))
    }
}

#[cfg(feature = "flowcatalyst")]
impl crate::db::TxBackend for TestTx {
    fn query(
        &self,
        sql: &str,
        params: &[crate::db::Param],
    ) -> Result<crate::db::Rows, crate::db::DbError> {
        self.databases
            .statement(&self.name, Some(self.id), sql, params, true)
            .map(rows_of)
    }

    fn execute(&self, sql: &str, params: &[crate::db::Param]) -> Result<u64, crate::db::DbError> {
        self.databases
            .statement(&self.name, Some(self.id), sql, params, false)
            .map(updated_of)
    }

    fn finish(mut self: Box<Self>, commit: bool) -> Result<(), crate::db::DbError> {
        self.over = true;
        let transaction = self.id;
        self.databases.events.borrow_mut().push(if commit {
            DbEvent::Commit { transaction }
        } else {
            DbEvent::Rollback { transaction }
        });
        Ok(())
    }
}

#[cfg(feature = "flowcatalyst")]
impl Drop for TestTx {
    fn drop(&mut self) {
        if !self.over {
            self.databases.events.borrow_mut().push(DbEvent::Dropped {
                transaction: self.id,
            });
        }
    }
}

#[cfg(all(feature = "flowcatalyst", feature = "json"))]
fn is_json(bytes: &[u8]) -> bool {
    serde_json::from_slice::<serde::de::IgnoredAny>(bytes).is_ok()
}

/// Without serde, only what cannot be JSON (not UTF-8) is caught.
#[cfg(all(feature = "flowcatalyst", not(feature = "json")))]
fn is_json(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_on;

    #[cfg(all(feature = "flowcatalyst", feature = "json"))]
    #[test]
    fn a_test_database_answers_and_records_what_the_function_did() {
        use crate::db::{DbError, DbErrorCode, Param};
        let host = TestHost::new().db("orders", |call| {
            if call.sql.starts_with("SELECT") {
                Ok(DbReply::rows(
                    serde_json::json!([{"id": 1, "state": "new"}]),
                ))
            } else if call.sql.contains("fail") {
                Err(DbError::new(DbErrorCode::Constraint, "duplicate key"))
            } else {
                Ok(DbReply::Updated(1))
            }
        });
        let ctx = host.context();
        let db = ctx.db("orders").unwrap();
        let rows = db
            .query(
                "SELECT id, state FROM orders WHERE id = ?",
                crate::params![1],
            )
            .unwrap();
        assert_eq!(rows.len(), 1);
        #[derive(serde::Deserialize, Debug, PartialEq)]
        struct Order {
            id: i64,
            state: String,
        }
        assert_eq!(
            rows.parse::<Order>().unwrap(),
            vec![Order {
                id: 1,
                state: "new".into()
            }]
        );
        let tx = db.begin().unwrap();
        assert_eq!(
            tx.execute("UPDATE orders SET state = ?", crate::params!["x"])
                .unwrap(),
            1
        );
        tx.commit().unwrap();
        let tx = db.begin().unwrap();
        assert_eq!(
            tx.execute("INSERT fail", &[]).unwrap_err().code(),
            "DB_CONSTRAINT"
        );
        drop(tx);
        let outcome: Result<(), DbError> = db.transaction(|tx| {
            tx.execute("DELETE FROM orders", &[])?;
            Err(DbError::new(DbErrorCode::Error, "changed my mind"))
        });
        assert!(outcome.is_err());
        assert_eq!(ctx.db("other").unwrap_err().code(), "DB_NOT_DECLARED");
        let events = host.db_events();
        assert_eq!(
            events[0],
            DbEvent::Statement {
                db: "orders".into(),
                transaction: None,
                sql: "SELECT id, state FROM orders WHERE id = ?".into(),
                params: vec![Param::Int(1)],
                query: true,
            }
        );
        let tail: Vec<_> = events[1..]
            .iter()
            .map(|e| match e {
                DbEvent::Statement { transaction, .. } => format!("stmt {transaction:?}"),
                DbEvent::Begin { transaction, .. } => format!("begin {transaction}"),
                DbEvent::Commit { transaction } => format!("commit {transaction}"),
                DbEvent::Rollback { transaction } => format!("rollback {transaction}"),
                DbEvent::Dropped { transaction } => format!("dropped {transaction}"),
            })
            .collect();
        assert_eq!(
            tail,
            [
                "begin 1",
                "stmt Some(1)",
                "commit 1",
                "begin 2",
                "stmt Some(2)",
                "dropped 2",
                "begin 3",
                "stmt Some(3)",
                "dropped 3"
            ]
        );
    }

    #[test]
    fn the_clock_and_the_logger_are_the_hosts() {
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(5);
        let host = TestHost::new().now(at);
        let ctx = host.context();
        assert_eq!(ctx.now(), at);
        ctx.logger().info("one");
        ctx.logger().error(format_args!("two {}", 2));
        assert_eq!(
            host.logs(),
            [
                (Level::Info, "one".to_string()),
                (Level::Error, "two 2".to_string())
            ]
        );
    }

    #[test]
    fn outbound_calls_are_denied_by_default_and_answered_by_the_responder() {
        let host = TestHost::new();
        let denied = block_on(host.context().http().get("https://api.example.com/x")).unwrap_err();
        assert_eq!(
            denied,
            HttpError::Denied(HttpDenied::new("api.example.com"))
        );

        let host = TestHost::new().http(|call| {
            Ok(HttpReply::new(
                200,
                MultiMap::new(),
                format!("{} {}", call.method(), call.url()),
            ))
        });
        let reply = block_on(
            host.context()
                .http()
                .send(HttpCall::post("https://a.test/b")),
        )
        .unwrap();
        assert_eq!(reply.text().unwrap(), "POST https://a.test/b");
        let bad = block_on(host.context().http().get("not a url")).unwrap_err();
        assert!(matches!(bad, HttpError::InvalidRequest(_)));
        let zero = block_on(
            host.context()
                .http()
                .send(HttpCall::get("https://a.test/").with_timeout(Duration::ZERO)),
        );
        assert!(matches!(zero, Err(HttpError::InvalidRequest(_))));
        assert_eq!(host.http_calls().len(), 3);
    }

    #[cfg(feature = "flowcatalyst")]
    #[test]
    fn config_and_secrets_answer_what_was_declared() {
        let host = TestHost::new()
            .config("GREETING", "hi")
            .secret("API_KEY", "k-1")
            .secret("EMPTY", "");
        let ctx = host.context();
        assert_eq!(ctx.config().get("GREETING").as_deref(), Some("hi"));
        assert_eq!(ctx.config().require("GREETING").unwrap(), "hi");
        assert_eq!(
            ctx.config().require("NOPE").unwrap_err().to_string(),
            "config key not declared: NOPE"
        );
        assert_eq!(ctx.secrets().require("API_KEY").unwrap(), "k-1");
        assert_eq!(ctx.secrets().get("EMPTY"), None);
        assert_eq!(
            ctx.secrets().require("EMPTY").unwrap_err().to_string(),
            "secret key not declared: EMPTY"
        );
    }

    #[cfg(feature = "flowcatalyst")]
    #[test]
    fn emit_applies_the_hosts_checks_and_defaults() {
        let host = TestHost::new().with_invocation(|i| {
            i.correlation_id = "corr-1".into();
            i.causation_id = Some("evt-0".into());
        });
        let events = host.context().events();
        let event = OutboundEvent::new("a:b:c:d", "d-1")
            .unwrap()
            .with_data(&b"{}"[..]);
        assert_eq!(events.emit(&event).unwrap(), "evt_1");
        assert_eq!(
            events
                .emit(&event.clone().with_correlation_id("mine"))
                .unwrap(),
            "evt_2"
        );
        let sent = host.emitted();
        assert_eq!(
            (sent[0].correlation_id(), sent[0].causation_id()),
            (Some("corr-1"), Some("evt-0"))
        );
        assert_eq!(sent[1].correlation_id(), Some("mine"));

        let blank_type = OutboundEvent::new(" ", "d-2").unwrap();
        assert_eq!(
            events.emit(&blank_type),
            Err(EmitError::Invalid("INVALID_EVENT: type is required".into()))
        );
        let not_json =
            OutboundEvent::new("a:b:c:d", "d-3")
                .unwrap()
                .with_data(if cfg!(feature = "json") {
                    &b"nope"[..]
                } else {
                    &b"\xff"[..]
                });
        assert_eq!(
            events.emit(&not_json).unwrap_err().code(),
            "INVALID_EVENT: data is not JSON"
        );
        assert_eq!(host.emitted().len(), 2);

        let refusing = TestHost::new().emit_with(|_| {
            Err(EmitError::Refused {
                code: "EVENT_TYPE_NOT_OWNED".into(),
                status: 403,
                message: "not yours".into(),
            })
        });
        let refused = refusing.context().events().emit(&event).unwrap_err();
        assert_eq!(
            (refused.code(), refused.status(), refused.message()),
            ("EVENT_TYPE_NOT_OWNED", 403, "not yours")
        );
        assert!(refusing.emitted().is_empty());
    }

    #[cfg(feature = "flowcatalyst")]
    #[test]
    #[should_panic(expected = "configure the TestHost before")]
    fn configuring_after_use_is_a_bug() {
        let host = TestHost::new();
        let _ctx = host.context();
        let _ = host.config("late", "x");
    }
}
