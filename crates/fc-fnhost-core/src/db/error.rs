//! Why a database call failed (Java `fnhost/wasm/DbFailure.java`): always a
//! value the guest reads, never a trap, and never carrying SQL text or a
//! parameter value. The codes are Java's.

use std::time::Duration;

use crate::log_throttle::LogThrottle;

/// The wire codes, Java's `DbFailure.code()` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DbErrorCode {
    /// `DB_NOT_DECLARED`: the name is not a manifest `db[].name`.
    NotDeclared,
    /// `DB_BAD_REQUEST`: the call itself is malformed.
    BadRequest,
    /// `DB_TX_UNKNOWN`: the transaction is not open.
    TxUnknown,
    /// `DB_CONSTRAINT`: SQLSTATE class 23.
    Constraint,
    /// `DB_SYNTAX`: SQLSTATE class 42.
    Syntax,
    /// `DB_TIMEOUT`: `57014`, a wait past the deadline, or no time left.
    Timeout,
    /// `DB_UNAVAILABLE`: classes 08, 53 and the rest of 57, or a
    /// connection-level failure with no SQLSTATE.
    Unavailable,
    /// `DB_ERROR`: everything else.
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

    /// Java `SqlClass.ofState`: `57014` is a timeout; then by class.
    pub fn of_sql_state(state: &str) -> DbErrorCode {
        if state == "57014" {
            return DbErrorCode::Timeout;
        }
        match state.get(..2) {
            Some("23") => DbErrorCode::Constraint,
            Some("42") => DbErrorCode::Syntax,
            Some("08") | Some("57") | Some("53") => DbErrorCode::Unavailable,
            _ => DbErrorCode::Error,
        }
    }
}

impl std::fmt::Display for DbErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A failed call: the code and a message for the guest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbFailure {
    pub code: DbErrorCode,
    pub message: String,
}

impl DbFailure {
    pub fn new(code: DbErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn not_declared(name: &str) -> Self {
        Self::new(
            DbErrorCode::NotDeclared,
            format!("no database named '{name}' is declared by this function's manifest"),
        )
    }

    pub fn bad_request(detail: impl Into<String>) -> Self {
        Self::new(DbErrorCode::BadRequest, detail)
    }

    pub fn tx_unknown() -> Self {
        Self::new(
            DbErrorCode::TxUnknown,
            "no open transaction with this id in this call",
        )
    }

    /// Less than a millisecond left: the statement is not sent.
    pub fn no_time_left() -> Self {
        Self::new(
            DbErrorCode::Timeout,
            "no time left before the invocation deadline",
        )
    }

    /// The wait for a connection (or for the function's share of the pool)
    /// reached the deadline (Java reports the interrupted wait as `57014`).
    pub fn no_connection_in_time() -> Self {
        Self::new(
            DbErrorCode::Timeout,
            "no connection became free before the invocation deadline",
        )
    }

    /// The pool had room, yet no connection opened before the deadline: the
    /// server refuses connections (sqlx retries a refused connect until the
    /// wait ends). Logged for the operator, as every unavailable database.
    pub fn unreachable(db: &str) -> Self {
        let failure = Self::new(
            DbErrorCode::Unavailable,
            "the database could not be reached before the invocation deadline",
        );
        operator_warning(db, failure.code, None, &failure.message);
        failure
    }

    /// The invocation is over (or not started): nothing may borrow a
    /// connection for it.
    pub fn no_invocation() -> Self {
        Self::new(
            DbErrorCode::Error,
            "database access is only available during a call",
        )
    }

    /// A driver failure, classified as Java's `SqlClass.of`, and logged for
    /// the operator when the database is unavailable (Java 529ec580): the
    /// database's name, the SQLSTATE and the driver's message, never SQL or
    /// a parameter. `db` is the manifest's name for it.
    pub fn from_sqlx(db: &str, error: &sqlx::Error) -> Self {
        let (code, state, message) = classify(error);
        let failure = Self::new(code, message);
        if code == DbErrorCode::Unavailable || state.as_deref().is_some_and(is_authorization) {
            operator_warning(db, code, state.as_deref(), &failure.message);
        }
        failure
    }
}

impl std::fmt::Display for DbFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

/// The code, the SQLSTATE when there is one, and the message.
fn classify(error: &sqlx::Error) -> (DbErrorCode, Option<String>, String) {
    match error {
        sqlx::Error::Database(e) => {
            let state = e.code().map(|c| c.into_owned());
            let code = state
                .as_deref()
                .filter(|s| s.len() >= 2)
                .map_or(DbErrorCode::Error, DbErrorCode::of_sql_state);
            (code, state, e.message().to_owned())
        }
        // Connection-kind failures with no SQLSTATE (Java: a transient or
        // non-transient connection exception).
        sqlx::Error::Io(e) => (
            DbErrorCode::Unavailable,
            None,
            format!("the database connection failed: {e}"),
        ),
        sqlx::Error::Tls(e) => (
            DbErrorCode::Unavailable,
            None,
            format!("the database TLS handshake failed: {e}"),
        ),
        sqlx::Error::PoolClosed => (
            DbErrorCode::Unavailable,
            None,
            "the database pool is closed".to_owned(),
        ),
        sqlx::Error::PoolTimedOut => (
            DbErrorCode::Timeout,
            None,
            "no connection became free in time".to_owned(),
        ),
        sqlx::Error::Configuration(e) => (
            DbErrorCode::Unavailable,
            None,
            format!("the database connection is misconfigured: {e}"),
        ),
        other => (DbErrorCode::Error, None, other.to_string()),
    }
}

/// SQLSTATE class 28 (invalid authorization, e.g. a password that rotated):
/// the guest gets `DB_ERROR`, as Java, and the operator a line.
fn is_authorization(state: &str) -> bool {
    state.starts_with("28")
}

static OPERATOR_LOG: LogThrottle = LogThrottle::new(Duration::from_secs(10));

fn operator_warning(db: &str, code: DbErrorCode, state: Option<&str>, message: &str) {
    if let Some(suppressed) = OPERATOR_LOG.admit() {
        tracing::warn!(
            db,
            code = code.as_str(),
            sql_state = state.unwrap_or(""),
            suppressed_since_last = suppressed,
            err = message,
            "a function's database is unavailable"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Java `DbFailureTest`'s pinned table.
    #[test]
    fn sql_states_map_to_javas_codes() {
        for (state, code) in [
            ("23505", DbErrorCode::Constraint),
            ("23503", DbErrorCode::Constraint),
            ("42601", DbErrorCode::Syntax),
            ("42P01", DbErrorCode::Syntax),
            ("57014", DbErrorCode::Timeout),
            ("57P01", DbErrorCode::Unavailable),
            ("08006", DbErrorCode::Unavailable),
            ("53300", DbErrorCode::Unavailable),
            ("40001", DbErrorCode::Error),
            ("22012", DbErrorCode::Error),
            ("25P02", DbErrorCode::Error),
            ("28P01", DbErrorCode::Error),
        ] {
            assert_eq!(DbErrorCode::of_sql_state(state), code, "{state}");
        }
    }

    #[test]
    fn the_wire_codes_are_javas() {
        let all = [
            (DbErrorCode::NotDeclared, "DB_NOT_DECLARED"),
            (DbErrorCode::BadRequest, "DB_BAD_REQUEST"),
            (DbErrorCode::TxUnknown, "DB_TX_UNKNOWN"),
            (DbErrorCode::Constraint, "DB_CONSTRAINT"),
            (DbErrorCode::Syntax, "DB_SYNTAX"),
            (DbErrorCode::Timeout, "DB_TIMEOUT"),
            (DbErrorCode::Unavailable, "DB_UNAVAILABLE"),
            (DbErrorCode::Error, "DB_ERROR"),
        ];
        for (code, wire) in all {
            assert_eq!(code.as_str(), wire);
        }
    }

    #[test]
    fn connection_failures_without_a_state_are_unavailable() {
        let io = sqlx::Error::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "refused",
        ));
        assert_eq!(
            DbFailure::from_sqlx("orders", &io).code,
            DbErrorCode::Unavailable
        );
        assert_eq!(
            DbFailure::from_sqlx("orders", &sqlx::Error::PoolClosed).code,
            DbErrorCode::Unavailable
        );
        assert_eq!(
            DbFailure::from_sqlx("orders", &sqlx::Error::Protocol("x".into())).code,
            DbErrorCode::Error
        );
    }
}
