//! The database state of ONE invocation (Java `fnhost/wasm/DbSession.java`):
//! the manifest's `db[]` pools, and the transactions this call has open.
//!
//! - **Scoped to the call.** A [`DbSession`] is made per invocation; the
//!   [`Database`] and [`Transaction`] handles it gives out live in that
//!   invocation's store, so no other call can reach them (Java's per-call
//!   transaction registry, by construction).
//! - **Force-release.** A [`Transaction`] dropped without a commit or a
//!   rollback (the guest dropped it, or the invocation ended, however it
//!   ended) returns its connection, and the pool's reset rolls the
//!   transaction back ([`super::pools::reset_session`]).
//! - **Without a transaction**, a statement borrows a connection and
//!   returns it before answering.
//! - **Deadline.** Every statement's `statement_timeout` is the time left
//!   before the invocation's deadline, to the millisecond; with less than
//!   1 ms left the statement is not sent. Every wait for a connection ends
//!   at the deadline too.
//! - **Caps.** A statement holds one of the function's share of the pool;
//!   an invocation holds at most [`PoolLease::per_invocation_cap`]
//!   transactions per database, and a statement outside them is refused
//!   rather than left waiting on the invocation's own transactions.
//! - SQL text and parameter values never reach a log line.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use futures::TryStreamExt;
use sqlx::pool::PoolConnection;
use sqlx::postgres::{PgArguments, PgConnection, PgTypeInfo};
use sqlx::{Arguments, Connection as _, Either, Executor as _, Postgres, Statement as _};

use super::error::DbFailure;
use super::params::{self, Param};
use super::placeholders;
use super::pools::{Borrow, PoolLease, SharePermit};
use super::rows::{RowWriter, RowsAnswer};

/// A version's databases, by `db[].name`.
pub type DbBindings = HashMap<String, Arc<PoolLease>>;

/// One invocation's databases.
pub struct DbSession {
    bindings: Option<Arc<DbBindings>>,
    deadline: Instant,
    /// Open transactions per database name.
    open: HashMap<String, Arc<AtomicUsize>>,
}

impl DbSession {
    pub fn new(bindings: Option<Arc<DbBindings>>, deadline: Instant) -> Self {
        Self {
            bindings,
            deadline,
            open: HashMap::new(),
        }
    }

    /// The manifest's `db[]` entry named `name`.
    pub fn open(&mut self, name: &str) -> Result<Database, DbFailure> {
        let lease = self
            .bindings
            .as_ref()
            .and_then(|b| b.get(name))
            .cloned()
            .ok_or_else(|| DbFailure::not_declared(name))?;
        let open = self.open.entry(name.to_owned()).or_default().clone();
        Ok(Database {
            name: name.to_owned(),
            lease,
            open,
            deadline: self.deadline,
        })
    }
}

/// A declared database, opened by the invocation.
pub struct Database {
    name: String,
    lease: Arc<PoolLease>,
    open: Arc<AtomicUsize>,
    deadline: Instant,
}

/// A borrowed connection with the share permit it holds.
struct Borrowed {
    conn: PoolConnection<Postgres>,
    share: SharePermit,
    count: Borrow,
}

impl Database {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// A statement that returns rows, on a connection of its own.
    pub async fn query(&self, sql: &str, params: &[Param]) -> Result<RowsAnswer, DbFailure> {
        check(sql)?;
        let mut borrowed = self.borrow(false).await?;
        let mut cache = Signatures::default();
        match run(
            &mut borrowed.conn,
            &self.name,
            sql,
            params,
            Mode::Query,
            self.deadline,
            &mut cache,
        )
        .await?
        {
            Outcome::Rows(rows) => Ok(rows),
            Outcome::Updated(_) => unreachable!("a query answers rows"),
        }
    }

    /// A statement for its effect, on a connection of its own; the rows it
    /// affected.
    pub async fn execute(&self, sql: &str, params: &[Param]) -> Result<u64, DbFailure> {
        check(sql)?;
        let mut borrowed = self.borrow(false).await?;
        let mut cache = Signatures::default();
        match run(
            &mut borrowed.conn,
            &self.name,
            sql,
            params,
            Mode::Execute,
            self.deadline,
            &mut cache,
        )
        .await?
        {
            Outcome::Updated(n) => Ok(n),
            Outcome::Rows(_) => unreachable!("an execute answers a count"),
        }
    }

    /// Opens a transaction on a connection it holds until it ends.
    pub async fn begin(&self) -> Result<Transaction, DbFailure> {
        let mut borrowed = self.borrow(true).await?;
        if remaining_ms(self.deadline).is_none() {
            return Err(DbFailure::no_time_left());
        }
        borrowed
            .conn
            .execute("BEGIN")
            .await
            .map_err(|e| DbFailure::from_sqlx(&self.name, &e))?;
        self.open.fetch_add(1, Ordering::SeqCst);
        Ok(Transaction {
            name: self.name.clone(),
            conn: Some(borrowed.conn),
            _share: borrowed.share,
            _count: borrowed.count,
            open: self.open.clone(),
            deadline: self.deadline,
            signatures: Signatures::default(),
        })
    }

    /// A connection and a share permit, both waited for no longer than the
    /// deadline. `for_transaction`: it will be held, so the per-invocation
    /// cap applies; otherwise only a wait on the invocation's own
    /// transactions is refused.
    async fn borrow(&self, for_transaction: bool) -> Result<Borrowed, DbFailure> {
        if remaining_ms(self.deadline).is_none() {
            return Err(DbFailure::no_time_left());
        }
        let held = self.open.load(Ordering::SeqCst);
        if for_transaction {
            let cap = self.lease.per_invocation_cap();
            if held >= cap {
                return Err(DbFailure::bad_request(format!(
                    "this invocation already holds {held} open transaction(s) on '{}', its most at once",
                    self.name
                )));
            }
        } else if held >= self.lease.share().size() {
            return Err(DbFailure::bad_request(format!(
                "this invocation's open transactions hold every connection its function may have on '{}'; run the statement in one of them",
                self.name
            )));
        }
        let deadline = tokio::time::Instant::from_std(self.deadline);
        let share = tokio::time::timeout_at(deadline, self.lease.share().acquire())
            .await
            .map_err(|_| DbFailure::no_connection_in_time())?;
        let entry = self.lease.entry();
        let pool = entry.pool();
        let conn = match tokio::time::timeout_at(deadline, pool.acquire()).await {
            Ok(conn) => conn.map_err(|e| DbFailure::from_sqlx(&self.name, &e))?,
            // Busy (every connection held) is a timeout; room in the pool
            // and still no connection is a database that would not connect.
            Err(_) if entry.borrowed() < entry.size() as usize => {
                return Err(DbFailure::unreachable(&self.name))
            }
            Err(_) => return Err(DbFailure::no_connection_in_time()),
        };
        Ok(Borrowed {
            conn,
            share,
            count: Borrow::new(entry),
        })
    }
}

/// An open transaction. Dropping it rolls it back.
pub struct Transaction {
    name: String,
    /// `None` once committed or rolled back.
    conn: Option<PoolConnection<Postgres>>,
    _share: SharePermit,
    _count: Borrow,
    open: Arc<AtomicUsize>,
    deadline: Instant,
    signatures: Signatures,
}

impl Transaction {
    pub async fn query(&mut self, sql: &str, params: &[Param]) -> Result<RowsAnswer, DbFailure> {
        check(sql)?;
        let conn = self.conn.as_mut().ok_or_else(DbFailure::tx_unknown)?;
        match run(
            conn,
            &self.name,
            sql,
            params,
            Mode::Query,
            self.deadline,
            &mut self.signatures,
        )
        .await?
        {
            Outcome::Rows(rows) => Ok(rows),
            Outcome::Updated(_) => unreachable!("a query answers rows"),
        }
    }

    pub async fn execute(&mut self, sql: &str, params: &[Param]) -> Result<u64, DbFailure> {
        check(sql)?;
        let conn = self.conn.as_mut().ok_or_else(DbFailure::tx_unknown)?;
        match run(
            conn,
            &self.name,
            sql,
            params,
            Mode::Execute,
            self.deadline,
            &mut self.signatures,
        )
        .await?
        {
            Outcome::Updated(n) => Ok(n),
            Outcome::Rows(_) => unreachable!("an execute answers a count"),
        }
    }

    /// Commits. The transaction is over whatever the outcome: a commit the
    /// server refuses (a deferred constraint), or of a transaction a
    /// statement already failed in, rolls it back, and the connection goes
    /// back to the pool either way.
    pub async fn commit(mut self) -> Result<(), DbFailure> {
        let mut conn = self.conn.take().ok_or_else(DbFailure::tx_unknown)?;
        let ms = remaining_ms(self.deadline).ok_or_else(DbFailure::no_time_left)?;
        // In a failed transaction PostgreSQL answers a plain COMMIT with
        // ROLLBACK and no error; the SET fails there first (25P02), so the
        // guest learns its transaction did not commit, as pgjdbc reports.
        conn.execute(format!("SET statement_timeout = {ms}; COMMIT").as_str())
            .await
            .map(|_| ())
            .map_err(|e| DbFailure::from_sqlx(&self.name, &e))
    }

    pub async fn rollback(mut self) -> Result<(), DbFailure> {
        let mut conn = self.conn.take().ok_or_else(DbFailure::tx_unknown)?;
        conn.execute("ROLLBACK")
            .await
            .map(|_| ())
            .map_err(|e| DbFailure::from_sqlx(&self.name, &e))
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        // The connection (if still held) goes back to the pool, whose reset
        // rolls the transaction back.
        self.open.fetch_sub(1, Ordering::SeqCst);
    }
}

fn check(sql: &str) -> Result<(), DbFailure> {
    if crate::java::is_blank(sql) {
        return Err(DbFailure::bad_request(
            "sql is required and must be a non-empty string",
        ));
    }
    Ok(())
}

/// Whole milliseconds left before `deadline`, rounded down; `None` under 1.
fn remaining_ms(deadline: Instant) -> Option<u64> {
    let ms = deadline
        .saturating_duration_since(Instant::now())
        .as_millis();
    (ms >= 1).then(|| u64::try_from(ms).unwrap_or(u64::MAX))
}

#[derive(Clone, Copy)]
enum Mode {
    Query,
    Execute,
}

enum Outcome {
    Rows(RowsAnswer),
    Updated(u64),
}

/// The statements prepared (and cached by sqlx) on one connection, with
/// the parameter types each was prepared with. sqlx caches by SQL text
/// alone, so the same text with other types clears the cache first.
#[derive(Default)]
struct Signatures(HashMap<String, Vec<u32>>);

/// One statement: its timeout, then prepare (the server infers what text
/// and `NULL` parameters are), bind each parameter as its placeholder's
/// type, run.
async fn run(
    conn: &mut PgConnection,
    db: &str,
    sql: &str,
    params: &[Param],
    mode: Mode,
    deadline: Instant,
    signatures: &mut Signatures,
) -> Result<Outcome, DbFailure> {
    let rewritten = placeholders::rewrite(sql);
    if rewritten.placeholders != params.len() {
        return Err(DbFailure::bad_request(format!(
            "the statement has {} placeholder(s) but {} param(s) were given",
            rewritten.placeholders,
            params.len()
        )));
    }
    let ms = remaining_ms(deadline).ok_or_else(DbFailure::no_time_left)?;
    let failed = |e: sqlx::Error| DbFailure::from_sqlx(db, &e);
    conn.execute(format!("SET statement_timeout = {ms}").as_str())
        .await
        .map_err(failed)?;

    let declared: Vec<PgTypeInfo> = params.iter().map(params::declared_type).collect();
    let signature: Vec<u32> = declared
        .iter()
        .map(|t| t.oid().map_or(0, |o| o.0))
        .collect();
    if signatures
        .0
        .get(&rewritten.sql)
        .is_some_and(|known| *known != signature)
    {
        conn.clear_cached_statements().await.map_err(failed)?;
        signatures.0.clear();
    }
    signatures.0.insert(rewritten.sql.clone(), signature);

    let statement = (&mut *conn)
        .prepare_with(&rewritten.sql, &declared)
        .await
        .map_err(failed)?;
    let types: Vec<PgTypeInfo> = match statement.parameters() {
        Some(Either::Left(types)) => types.to_vec(),
        _ => Vec::new(),
    };
    if types.len() != params.len() {
        return Err(DbFailure::bad_request(format!(
            "the statement takes {} parameter(s) but {} were given",
            types.len(),
            params.len()
        )));
    }
    let mut arguments = PgArguments::default();
    for (i, (param, ty)) in params.iter().zip(&types).enumerate() {
        let bound = params::bind(i, param, ty).map_err(DbFailure::bad_request)?;
        arguments.add(bound).map_err(|e| {
            DbFailure::bad_request(format!("params[{i}] could not be encoded: {e}"))
        })?;
    }
    let query = sqlx::query_with(&rewritten.sql, arguments);
    match mode {
        Mode::Query => {
            let mut rows = conn.fetch(query);
            let mut writer = RowWriter::default();
            // Stops reading at the cap; the rest of the result is drained
            // before the connection's next use (or its reset).
            while let Some(row) = rows.try_next().await.map_err(failed)? {
                if !writer.push(&row) {
                    break;
                }
            }
            Ok(Outcome::Rows(writer.finish()))
        }
        Mode::Execute => conn
            .execute(query)
            .await
            .map(|done| Outcome::Updated(done.rows_affected()))
            .map_err(failed),
    }
}
