//! The host's database pools (Java `fnhost/context/DbPools.java`, owner
//! decision #7): **one shared, bounded sqlx pool per connection identity**,
//! shared by every loaded function version that declares that database.
//!
//! - **Reference-counted by loaded version.** A version joins (or opens) the
//!   pool at load, eagerly, as Java: a DSN this host cannot use is a load
//!   failure (`DB_UNSUPPORTED`), not a surprise at the first query. No
//!   connection is opened at load (the pool is lazy), so a database outage
//!   is `DB_UNAVAILABLE` at run time, never a failed load. The pool closes
//!   when the last version using it unloads.
//! - **Sized to the largest `db[].poolSize`** among its current users (each
//!   already clamped to its client's ceiling by the platform). A sqlx pool
//!   cannot be resized, so a size change replaces it: new borrows go to the
//!   new pool, and the old one closes once its connections come back.
//! - **At most `FC_FN_MAX_DB_POOLS`** (default 16) distinct pools; one more
//!   is the load failure `DB_POOL_LIMIT`, never an eviction of a pool in use.
//! - **A per-function share** ([`ShareGate`]): one function holds at most
//!   its own `db[].poolSize` connections of a shared pool at once, across
//!   all of its versions and invocations, however slow its callers. This is
//!   the owner's share-limit hook: `share_limit` below is the one place to
//!   change the policy (for example to leave headroom when a pool is
//!   shared).
//! - **Every connection goes back clean** ([`reset_session`], sqlx's
//!   `after_release`): an open or failed transaction is rolled back and the
//!   session reset with `DISCARD ALL`, whatever the function's SQL did
//!   (`BEGIN` as a statement, `SET search_path`, a temporary table, an
//!   advisory lock). Java's pooled connections could go back mid-transaction
//!   (its backlog item of 2026-09-24); these cannot.
//! - **Secret-manager references are re-read** every
//!   `FC_FN_DB_SECRET_REFRESH_SECONDS` (default 300; 0 turns it off), and a
//!   changed connection reaches the pool's connect options, so a rotated RDS
//!   password never strands a pool (the `start_secret_refresh` rule of the
//!   platform's own pools). A literal DSN rotates through the platform: the
//!   secret's new value reloads the function, whose new identity opens a new
//!   pool.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use fc_function_abi::FunctionAddress;
use parking_lot::{Mutex, RwLock};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPool, PgPoolOptions};
use sqlx::{Connection as _, Executor as _};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use super::dsn::{pool_id, Dsn, DsnSource, SecretResolver, DB_SECRET_UNRESOLVED, DB_UNSUPPORTED};
use futures::future::BoxFuture;
use std::fmt;
use std::fmt::Formatter;
use std::mem;
use tokio::runtime::Handle;
use tokio::time;

/// The load-failure code for one pool too many (Java's).
pub const DB_POOL_LIMIT: &str = "DB_POOL_LIMIT";

/// An idle connection is closed after this long.
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// A connection is replaced after this long (credentials rotate, servers
/// fail over).
const MAX_LIFETIME: Duration = Duration::from_secs(30 * 60);
/// sqlx's own wait for a connection. Every wait is also bounded by the
/// invocation's deadline, which is always sooner.
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(300);

/// What the pools are configured with (`FC_FN_*`).
#[derive(Debug, Clone)]
pub struct DbSettings {
    /// `FC_FN_MAX_DB_POOLS` (default 16).
    pub max_pools: usize,
    /// `FC_FN_DB_MAX_CONNECTIONS_PER_INVOCATION` (default 2): connections
    /// one invocation may hold at once on one database, never more than the
    /// function's share. A transaction holds one until it ends; a statement
    /// outside one holds one only while it runs.
    pub max_connections_per_invocation: usize,
    /// `FC_FN_DB_SECRET_REFRESH_SECONDS` (default 300): how often a
    /// secret-manager reference is re-read; zero never.
    pub secret_refresh: Duration,
}

impl Default for DbSettings {
    fn default() -> Self {
        Self {
            max_pools: 16,
            max_connections_per_invocation: 2,
            secret_refresh: Duration::from_secs(300),
        }
    }
}

/// Why a version could not join a pool: a heartbeat load failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinFailure {
    pub code: &'static str,
    pub detail: String,
}

pub struct DbPools {
    settings: DbSettings,
    resolver: Arc<dyn SecretResolver>,
    pools: Mutex<HashMap<String, Arc<PoolEntry>>>,
    next_token: AtomicU64,
    me: Weak<DbPools>,
}

/// One shared pool.
pub struct PoolEntry {
    identity: String,
    /// A short, non-reversible name for logs.
    id: String,
    options: RwLock<PgConnectOptions>,
    pool: RwLock<PgPool>,
    state: Mutex<EntryState>,
    /// Connections functions hold right now (see [`Borrow`]).
    borrowed: AtomicUsize,
}

/// One connection a function holds, counted on its pool while it lives:
/// a wait that times out with the pool below its size means the database
/// never gave a connection (unreachable), not that the pool was busy.
pub struct Borrow(Arc<PoolEntry>);

impl Borrow {
    pub fn new(entry: &Arc<PoolEntry>) -> Self {
        entry.borrowed.fetch_add(1, Ordering::SeqCst);
        Self(entry.clone())
    }
}

impl Drop for Borrow {
    fn drop(&mut self) {
        self.0.borrowed.fetch_sub(1, Ordering::SeqCst);
    }
}

#[derive(Default)]
struct EntryState {
    size: u32,
    /// `token → (function, its db[].poolSize)`.
    users: HashMap<u64, (FunctionAddress, u32)>,
    shares: HashMap<FunctionAddress, Arc<ShareGate>>,
    closed: bool,
}

/// One version's membership of a pool, for one `db[]` entry. Dropping it
/// leaves the pool (closing it with its last user).
pub struct PoolLease {
    pools: Weak<DbPools>,
    entry: Arc<PoolEntry>,
    share: Arc<ShareGate>,
    token: u64,
    pool_size: u32,
    max_connections_per_invocation: usize,
}

impl DbPools {
    pub fn new(settings: DbSettings, resolver: Arc<dyn SecretResolver>) -> Arc<Self> {
        Arc::new_cyclic(|me| Self {
            settings,
            resolver,
            pools: Mutex::new(HashMap::new()),
            next_token: AtomicU64::new(1),
            me: me.clone(),
        })
    }

    pub fn settings(&self) -> &DbSettings {
        &self.settings
    }

    /// Joins, or opens, the pool the secret value `secret` names, for one
    /// `db[]` entry of a version of `address` declaring `pool_size`.
    pub async fn join(
        &self,
        address: &FunctionAddress,
        secret: &str,
        pool_size: u32,
    ) -> Result<PoolLease, JoinFailure> {
        let pool_size = pool_size.max(1);
        let source = DsnSource::of(secret, self.resolver.as_ref());
        let (identity, dsn) = match &source {
            DsnSource::Literal(raw) => {
                let dsn = Dsn::parse(raw).map_err(|detail| JoinFailure {
                    code: DB_UNSUPPORTED,
                    detail,
                })?;
                (dsn.identity().to_owned(), Some(dsn))
            }
            DsnSource::Reference(reference) => (format!("ref|{reference}"), None),
        };
        // A reference is read at every join (loads are rare), so a version
        // loading onto a pool its predecessor opened proves the secret
        // still reads.
        let dsn = match (dsn, &source) {
            (Some(dsn), _) => dsn,
            (None, DsnSource::Reference(reference)) => {
                let value =
                    self.resolver
                        .resolve(reference)
                        .await
                        .map_err(|detail| JoinFailure {
                            code: DB_SECRET_UNRESOLVED,
                            detail,
                        })?;
                Dsn::parse(&value).map_err(|detail| JoinFailure {
                    code: DB_UNSUPPORTED,
                    detail: format!("{reference}: {detail}"),
                })?
            }
            (None, DsnSource::Literal(_)) => unreachable!("a literal is parsed above"),
        };

        let token = self.next_token.fetch_add(1, Ordering::Relaxed);
        // Joined under the map's lock, so a concurrent last `leave` cannot
        // close the pool between finding it and joining it.
        let (entry, share) = {
            let mut pools = self.pools.lock();
            let entry = match pools.get(&identity) {
                Some(entry) => entry.clone(),
                None => {
                    if pools.len() >= self.settings.max_pools {
                        return Err(JoinFailure {
                            code: DB_POOL_LIMIT,
                            detail: format!(
                                "at most {} distinct database pools may be open on this host",
                                self.settings.max_pools
                            ),
                        });
                    }
                    let entry = Arc::new(PoolEntry {
                        id: pool_id(&identity),
                        identity: identity.clone(),
                        options: RwLock::new(dsn.connect_options().clone()),
                        pool: RwLock::new(new_pool(dsn.connect_options().clone(), pool_size)),
                        state: Mutex::new(EntryState {
                            size: pool_size,
                            ..EntryState::default()
                        }),
                        borrowed: AtomicUsize::new(0),
                    });
                    tracing::info!(pool = %entry.id, dsn = %dsn, size = pool_size, "function database pool opened");
                    if let DsnSource::Reference(reference) = &source {
                        self.start_refresh(&entry, reference.clone(), dsn.identity().to_owned());
                    }
                    pools.insert(identity.clone(), entry.clone());
                    entry
                }
            };
            let share = entry.add_user(token, address, pool_size);
            (entry, share)
        };
        Ok(PoolLease {
            pools: self.me.clone(),
            entry,
            share,
            token,
            pool_size,
            max_connections_per_invocation: self.settings.max_connections_per_invocation.max(1),
        })
    }

    fn leave(&self, identity: &str, token: u64) {
        let mut pools = self.pools.lock();
        let Some(entry) = pools.get(identity).cloned() else {
            return;
        };
        if entry.remove_user(token) {
            pools.remove(identity);
            tracing::info!(pool = %entry.id, "function database pool closed: its last user unloaded");
        }
    }

    /// Re-reads `reference` every `secret_refresh` while the pool lives,
    /// and hands a changed connection to the pool.
    fn start_refresh(&self, entry: &Arc<PoolEntry>, reference: String, mut current: String) {
        let interval = self.settings.secret_refresh;
        if interval.is_zero() {
            return;
        }
        let Ok(runtime) = Handle::try_current() else {
            return;
        };
        let weak = Arc::downgrade(entry);
        let resolver = self.resolver.clone();
        runtime.spawn(async move {
            loop {
                time::sleep(interval).await;
                let Some(entry) = weak.upgrade() else {
                    return;
                };
                if entry.state.lock().closed {
                    return;
                }
                match resolver.resolve(&reference).await.and_then(|v| Dsn::parse(&v)) {
                    Ok(dsn) if dsn.identity() != current => {
                        entry.set_connect_options(dsn.connect_options().clone());
                        current = dsn.identity().to_owned();
                        tracing::info!(pool = %entry.id, reference = %reference, "function database credentials changed: pool connect options updated");
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!(pool = %entry.id, reference = %reference, err = %e, "re-reading a function database's secret failed; the pool keeps its current credentials"),
                }
            }
        });
    }

    /// Open pools (a test and diagnostic seam, Java's `poolCountForTest`).
    pub fn pool_count(&self) -> usize {
        self.pools.lock().len()
    }
}

impl fmt::Debug for DbPools {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "DbPools[{} open]", self.pool_count())
    }
}

fn new_pool(options: PgConnectOptions, size: u32) -> PgPool {
    PgPoolOptions::new()
        .max_connections(size)
        .min_connections(0)
        .acquire_timeout(ACQUIRE_TIMEOUT)
        .idle_timeout(Some(IDLE_TIMEOUT))
        .max_lifetime(Some(MAX_LIFETIME))
        // Tested on the way back (`after_release`, then sqlx's ping)
        // instead of on the way out: one round trip fewer per borrow.
        .test_before_acquire(false)
        .after_release(|conn, _| reset_on_release(conn))
        .connect_lazy_with(options)
}

fn reset_on_release(conn: &mut PgConnection) -> BoxFuture<'_, Result<bool, sqlx::Error>> {
    Box::pin(async move { reset_session(conn).await.map(|()| true) })
}

/// A connection on its way back to the pool: roll back whatever
/// transaction is open or failed, reset the session (`DISCARD ALL`:
/// settings, temporary tables, prepared statements, cursors, advisory
/// locks, `LISTEN`s), and forget the statements sqlx had prepared on it. An
/// error closes the connection instead of returning it.
pub async fn reset_session(conn: &mut PgConnection) -> Result<(), sqlx::Error> {
    // `Executor::execute` on the connection itself (a `&str` with no
    // arguments is a simple query): `raw_sql`'s generic executor trips
    // rustc's higher-ranked `Send` check inside sqlx's `after_release`.
    match conn.execute("DISCARD ALL").await {
        Ok(_) => {}
        // 25001: in a transaction; 25P02: in a failed one.
        Err(sqlx::Error::Database(e))
            if matches!(e.code().as_deref(), Some("25001") | Some("25P02")) =>
        {
            conn.execute("ROLLBACK").await?;
            conn.execute("DISCARD ALL").await?;
        }
        Err(e) => return Err(e),
    }
    conn.clear_cached_statements().await
}

impl PoolEntry {
    /// The pool to borrow from now.
    pub fn pool(&self) -> PgPool {
        self.pool.read().clone()
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// The pool's size: the largest `poolSize` of its users.
    pub fn size(&self) -> u32 {
        self.state.lock().size
    }

    /// Connections functions hold right now.
    pub fn borrowed(&self) -> usize {
        self.borrowed.load(Ordering::SeqCst)
    }

    fn set_connect_options(&self, options: PgConnectOptions) {
        *self.options.write() = options.clone();
        self.pool.read().set_connect_options(options);
    }

    #[expect(
        clippy::expect_used,
        reason = "the user was inserted into the map just above, under the same lock"
    )]
    fn add_user(&self, token: u64, address: &FunctionAddress, pool_size: u32) -> Arc<ShareGate> {
        let mut state = self.state.lock();
        state.users.insert(token, (address.clone(), pool_size));
        self.resize(&mut state);
        self.reshare(&mut state, address)
            .expect("the address has a user: this one")
    }

    /// `true` once no user remains (the pool is closed).
    fn remove_user(&self, token: u64) -> bool {
        let mut state = self.state.lock();
        let Some((address, _)) = state.users.remove(&token) else {
            return false;
        };
        self.reshare(&mut state, &address);
        if state.users.is_empty() {
            state.closed = true;
            close_in_background(self.pool.read().clone());
            return true;
        }
        self.resize(&mut state);
        false
    }

    fn resize(&self, state: &mut EntryState) {
        let size = state.users.values().map(|u| u.1).max().unwrap_or(1).max(1);
        // A sqlx pool starts its housekeeping on the current runtime; with
        // none (a lease dropped outside one) the pool keeps its size.
        if size != state.size && Handle::try_current().is_ok() {
            let options = self.options.read().clone();
            let old = mem::replace(&mut *self.pool.write(), new_pool(options, size));
            close_in_background(old);
            tracing::info!(pool = %self.id, from = state.size, to = size, "function database pool resized");
            state.size = size;
        }
    }

    /// The share gate of `address`, sized to [`share_limit`] of its users'
    /// `poolSize`s; removed (`None`) once it has none.
    fn reshare(&self, state: &mut EntryState, address: &FunctionAddress) -> Option<Arc<ShareGate>> {
        let declared = state
            .users
            .values()
            .filter(|(a, _)| a == address)
            .map(|u| u.1)
            .max();
        let Some(declared) = declared else {
            state.shares.remove(address);
            return None;
        };
        let limit = share_limit(declared) as usize;
        let gate = state
            .shares
            .entry(address.clone())
            .or_insert_with(|| Arc::new(ShareGate::new(limit)))
            .clone();
        gate.resize(limit);
        Some(gate)
    }
}

/// How many connections of a shared pool one function may hold at once:
/// its own `db[].poolSize` (the largest over its loaded versions). The one
/// place the share policy lives (owner decision #7's hook).
fn share_limit(declared_pool_size: u32) -> u32 {
    declared_pool_size.max(1)
}

fn close_in_background(pool: PgPool) {
    if let Ok(runtime) = Handle::try_current() {
        runtime.spawn(async move { pool.close().await });
    }
}

impl PoolLease {
    pub fn entry(&self) -> &Arc<PoolEntry> {
        &self.entry
    }

    /// This function's share of the pool.
    pub fn share(&self) -> &Arc<ShareGate> {
        &self.share
    }

    /// The `db[].poolSize` this version declared.
    pub fn pool_size(&self) -> u32 {
        self.pool_size
    }

    /// Connections one invocation may hold at once on this database: the
    /// host's per-invocation cap, never more than the function's share (an
    /// invocation holding the whole share would wait on itself).
    pub fn per_invocation_cap(&self) -> usize {
        self.max_connections_per_invocation
            .min(self.share.size())
            .max(1)
    }
}

impl Drop for PoolLease {
    fn drop(&mut self) {
        if let Some(pools) = self.pools.upgrade() {
            pools.leave(&self.entry.identity, self.token);
        }
    }
}

impl fmt::Debug for PoolLease {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PoolLease[pool {}, poolSize {}]",
            self.entry.id, self.pool_size
        )
    }
}

/// A semaphore whose size can change while permits are out: growing adds
/// permits; shrinking forgets free ones at once and the rest as they come
/// back.
pub struct ShareGate {
    semaphore: Arc<Semaphore>,
    /// `(size, permits still to forget)`.
    state: Mutex<(usize, usize)>,
}

impl ShareGate {
    pub fn new(size: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(size)),
            state: Mutex::new((size, 0)),
        }
    }

    pub fn size(&self) -> usize {
        self.state.lock().0
    }

    /// Permits free now (a test seam).
    pub fn available(&self) -> usize {
        self.semaphore.available_permits()
    }

    pub fn resize(&self, size: usize) {
        let mut state = self.state.lock();
        let (current, debt) = *state;
        if size > current {
            let grow = size - current;
            let repaid = grow.min(debt);
            self.semaphore.add_permits(grow - repaid);
            *state = (size, debt - repaid);
        } else if size < current {
            let shrink = current - size;
            let forgotten = self.semaphore.forget_permits(shrink);
            *state = (size, debt + shrink - forgotten);
        }
    }

    /// Waits for a permit (cancel-safe).
    #[expect(
        clippy::expect_used,
        reason = "the share semaphore is never closed while the gate lives"
    )]
    pub async fn acquire(self: &Arc<Self>) -> SharePermit {
        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("the share semaphore is never closed");
        SharePermit {
            gate: self.clone(),
            permit: Some(permit),
        }
    }
}

/// One connection's worth of a function's share, held while the
/// connection is.
pub struct SharePermit {
    gate: Arc<ShareGate>,
    permit: Option<OwnedSemaphorePermit>,
}

impl Drop for SharePermit {
    fn drop(&mut self) {
        let mut state = self.gate.state.lock();
        if let Some(permit) = self.permit.take() {
            if state.1 > 0 {
                state.1 -= 1;
                permit.forget();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_share_gate_grows_at_once_and_shrinks_as_permits_come_back() {
        let gate = Arc::new(ShareGate::new(2));
        let a = gate.acquire().await;
        let b = gate.acquire().await;
        assert_eq!(gate.available(), 0);
        gate.resize(1);
        drop(a); // forgotten: the gate is now 1 with 1 out
        assert_eq!(gate.available(), 0);
        drop(b);
        assert_eq!(gate.available(), 1);
        gate.resize(3);
        assert_eq!(gate.available(), 3);
        gate.resize(1);
        assert_eq!(gate.available(), 1);
        assert_eq!(gate.size(), 1);
    }

    #[tokio::test]
    async fn pools_are_shared_by_identity_sized_to_the_largest_user_and_counted() {
        let pools = DbPools::new(
            DbSettings {
                max_pools: 2,
                ..DbSettings::default()
            },
            Arc::new(super::super::dsn::NoResolver),
        );
        let f = FunctionAddress::parse("app.svc.f").unwrap();
        let g = FunctionAddress::parse("app.svc.g").unwrap();
        let a = pools.join(&f, "postgres://u:p@db1/x", 2).await.unwrap();
        let b = pools
            .join(&g, "jdbc:postgresql://db1:5432/x?user=u&password=p", 5)
            .await
            .unwrap();
        assert_eq!(pools.pool_count(), 1, "one pool for one identity");
        assert!(Arc::ptr_eq(a.entry(), b.entry()));
        assert_eq!(a.entry().size(), 5, "sized to the largest poolSize");
        assert_eq!(a.share().size(), 2, "each function's own share");
        assert_eq!(b.share().size(), 5);
        let _c = pools.join(&f, "postgres://u:p@db2/x", 1).await.unwrap();
        let err = pools.join(&f, "postgres://u:p@db3/x", 1).await.unwrap_err();
        assert_eq!(err.code, DB_POOL_LIMIT);
        drop(b);
        assert_eq!(a.entry().size(), 2, "shrunk to the remaining user");
        let entry = a.entry().clone();
        drop(a);
        assert_eq!(pools.pool_count(), 1, "closed with its last user");
        assert!(entry.state.lock().closed);
        let err = pools.join(&f, "mysql://u:p@db/x", 1).await.unwrap_err();
        assert_eq!(err.code, DB_UNSUPPORTED);
    }

    #[tokio::test]
    async fn two_versions_of_one_function_share_one_gate_sized_to_the_larger() {
        let pools = DbPools::new(
            DbSettings::default(),
            Arc::new(super::super::dsn::NoResolver),
        );
        let f = FunctionAddress::parse("app.svc.f").unwrap();
        let v1 = pools.join(&f, "postgres://u:p@db/x", 2).await.unwrap();
        let v2 = pools.join(&f, "postgres://u:p@db/x", 3).await.unwrap();
        assert!(Arc::ptr_eq(v1.share(), v2.share()));
        assert_eq!(v1.share().size(), 3);
        assert_eq!(
            v1.per_invocation_cap(),
            2,
            "the host's cap, below the share"
        );
        drop(v2);
        assert_eq!(v1.share().size(), 2);
    }
}
