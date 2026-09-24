//! Use Case Result Type
//!
//! Sealed result type for use case execution. `success()` is `pub(crate)`,
//! so the only code that can construct a successful result is the SDK's own
//! `UnitOfWork` implementations (`OutboxUnitOfWork`,
//! `TxScopedOutboxUnitOfWork`, `InMemoryUnitOfWork`). Consumer use cases
//! defined outside the SDK MUST route success through one of these via
//! `unit_of_work.commit()` / `commit_delete()` / `emit_event()` /
//! `commit_all()`. This mirrors the platform's `pub(in crate::usecase)` seal
//! and the TS SDK's `Sealed<E>` pattern — compile-time enforced, zero cost.
//!
//! Trade-off: consumer apps that need a custom `UnitOfWork` backend (e.g.
//! MySQL / SQLite via sqlx, or a Diesel-based outbox) cannot construct
//! success directly. Open an issue if you hit this; the bundled backends
//! cover the common cases (Postgres outbox + in-memory for tests).
//!
//! Neither the constructor nor the tuple field is reachable from outside:
//!
//! ```compile_fail
//! use fc_sdk::usecase::UseCaseResult;
//! let forged: UseCaseResult<i32> = UseCaseResult::success(1);
//! ```
//!
//! ```compile_fail
//! use fc_sdk::usecase::UseCaseResult;
//! let forged: UseCaseResult<i32> = UseCaseResult(Ok(1));
//! ```

use super::error::UseCaseError;

/// Result type for use case execution.
///
/// A newtype over `Result<T, UseCaseError>` with a private field, so the only
/// way to build a success outside this crate is to get one from a
/// `UnitOfWork` (and optionally transform it with [`map`](Self::map) /
/// [`and_then`](Self::and_then)). Failures can be built anywhere with
/// [`failure`](Self::failure).
///
/// # Usage
///
/// ```ignore
/// // Return failure for validation/business rule violations
/// if !is_valid {
///     return UseCaseResult::failure(UseCaseError::validation("INVALID", "Invalid input"));
/// }
///
/// // Return success through UnitOfWork.commit()
/// unit_of_work.commit(aggregate, event, command).await
/// ```
#[must_use]
pub struct UseCaseResult<T>(Result<T, UseCaseError>);

impl<T> UseCaseResult<T> {
    /// Create a failure result.
    pub fn failure(error: UseCaseError) -> Self {
        Self(Err(error))
    }

    /// Create a success result.
    ///
    /// Sealed to `pub(crate)` — only the SDK's own `UnitOfWork` impls can
    /// construct a success. Consumer use cases must route through
    /// `unit_of_work.commit()` / `commit_delete()` / `emit_event()` /
    /// `commit_all()` (or `.map()` chained onto one of those). See the
    /// module-level documentation for the rationale.
    pub(crate) fn success(value: T) -> Self {
        Self(Ok(value))
    }

    pub fn is_success(&self) -> bool {
        self.0.is_ok()
    }

    pub fn is_failure(&self) -> bool {
        self.0.is_err()
    }

    /// Borrow the outcome as a plain `Result`.
    pub fn as_result(&self) -> Result<&T, &UseCaseError> {
        self.0.as_ref()
    }

    pub fn unwrap(self) -> T {
        match self.0 {
            Ok(v) => v,
            Err(e) => panic!("Called unwrap on a Failure: {}", e),
        }
    }

    pub fn unwrap_or(self, default: T) -> T {
        self.0.unwrap_or(default)
    }

    pub fn unwrap_or_else<F>(self, f: F) -> T
    where
        F: FnOnce(UseCaseError) -> T,
    {
        self.0.unwrap_or_else(f)
    }

    pub fn unwrap_err(self) -> UseCaseError {
        match self.0 {
            Ok(_) => panic!("Called unwrap_err on a Success"),
            Err(e) => e,
        }
    }

    pub fn as_ref(&self) -> UseCaseResult<&T> {
        UseCaseResult(self.0.as_ref().map_err(Clone::clone))
    }

    pub fn map<U, F>(self, f: F) -> UseCaseResult<U>
    where
        F: FnOnce(T) -> U,
    {
        UseCaseResult(self.0.map(f))
    }

    /// Chain another step that itself produces a `UseCaseResult` (for example
    /// a second use case). A success can still only originate from a
    /// `UnitOfWork`.
    pub fn and_then<U, F>(self, f: F) -> UseCaseResult<U>
    where
        F: FnOnce(T) -> UseCaseResult<U>,
    {
        match self.0 {
            Ok(v) => f(v),
            Err(e) => UseCaseResult(Err(e)),
        }
    }

    pub fn map_err<F>(self, f: F) -> UseCaseResult<T>
    where
        F: FnOnce(UseCaseError) -> UseCaseError,
    {
        UseCaseResult(self.0.map_err(f))
    }

    pub fn into_result(self) -> Result<T, UseCaseError> {
        self.0
    }
}

impl<T> From<UseCaseResult<T>> for Result<T, UseCaseError> {
    fn from(result: UseCaseResult<T>) -> Self {
        result.into_result()
    }
}

impl<T> From<UseCaseError> for UseCaseResult<T> {
    fn from(error: UseCaseError) -> Self {
        Self::failure(error)
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for UseCaseResult<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Ok(v) => f.debug_tuple("Success").field(v).finish(),
            Err(e) => f.debug_tuple("Failure").field(e).finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_creation() {
        let result: UseCaseResult<i32> = UseCaseResult::success(42);
        assert!(result.is_success());
        assert!(!result.is_failure());
    }

    #[test]
    fn failure_creation() {
        let result: UseCaseResult<i32> =
            UseCaseResult::failure(UseCaseError::validation("V", "bad input"));
        assert!(result.is_failure());
        assert!(!result.is_success());
    }

    #[test]
    fn unwrap_success() {
        let result = UseCaseResult::success("hello");
        assert_eq!(result.unwrap(), "hello");
    }

    #[test]
    #[should_panic(expected = "Called unwrap on a Failure")]
    fn unwrap_failure_panics() {
        let result: UseCaseResult<()> =
            UseCaseResult::failure(UseCaseError::validation("V", "fail"));
        result.unwrap();
    }

    #[test]
    fn unwrap_or_returns_value_on_success() {
        let result = UseCaseResult::success(10);
        assert_eq!(result.unwrap_or(0), 10);
    }

    #[test]
    fn unwrap_or_returns_default_on_failure() {
        let result: UseCaseResult<i32> =
            UseCaseResult::failure(UseCaseError::not_found("NF", "gone"));
        assert_eq!(result.unwrap_or(99), 99);
    }

    #[test]
    fn unwrap_or_else_success() {
        let result = UseCaseResult::success(5);
        let val = result.unwrap_or_else(|_| 0);
        assert_eq!(val, 5);
    }

    #[test]
    fn unwrap_or_else_failure() {
        let result: UseCaseResult<String> =
            UseCaseResult::failure(UseCaseError::validation("V", "msg"));
        let val = result.unwrap_or_else(|e| format!("error: {}", e.code()));
        assert_eq!(val, "error: V");
    }

    #[test]
    fn unwrap_err_on_failure() {
        let result: UseCaseResult<()> = UseCaseResult::failure(UseCaseError::commit("db error"));
        let err = result.unwrap_err();
        assert_eq!(err.code(), "COMMIT_FAILED");
    }

    #[test]
    #[should_panic(expected = "Called unwrap_err on a Success")]
    fn unwrap_err_on_success_panics() {
        let result = UseCaseResult::success(42);
        result.unwrap_err();
    }

    #[test]
    fn map_transforms_success() {
        let result = UseCaseResult::success(10);
        let mapped = result.map(|v| v * 2);
        assert_eq!(mapped.unwrap(), 20);
    }

    #[test]
    fn map_preserves_failure() {
        let result: UseCaseResult<i32> = UseCaseResult::failure(UseCaseError::not_found("NF", "x"));
        let mapped = result.map(|v| v * 2);
        assert!(mapped.is_failure());
        assert_eq!(mapped.unwrap_err().code(), "NF");
    }

    #[test]
    fn map_err_transforms_failure() {
        let result: UseCaseResult<i32> =
            UseCaseResult::failure(UseCaseError::validation("OLD", "old msg"));
        let mapped = result.map_err(|_| UseCaseError::business_rule("NEW", "new msg"));
        assert_eq!(mapped.unwrap_err().code(), "NEW");
    }

    #[test]
    fn map_err_preserves_success() {
        let result = UseCaseResult::success(42);
        let mapped = result.map_err(|_| UseCaseError::commit("should not happen"));
        assert_eq!(mapped.unwrap(), 42);
    }

    #[test]
    fn into_result_success() {
        let result = UseCaseResult::success("ok");
        let std_result: Result<&str, UseCaseError> = result.into_result();
        assert_eq!(std_result.unwrap(), "ok");
    }

    #[test]
    fn into_result_failure() {
        let result: UseCaseResult<()> =
            UseCaseResult::failure(UseCaseError::concurrency("STALE", "retry"));
        let std_result: Result<(), UseCaseError> = result.into_result();
        assert!(std_result.is_err());
        assert_eq!(std_result.unwrap_err().code(), "STALE");
    }

    #[test]
    fn from_impl_conversion() {
        let result = UseCaseResult::success(100);
        let converted: Result<i32, UseCaseError> = result.into();
        assert_eq!(converted.unwrap(), 100);
    }

    #[test]
    fn as_ref_success() {
        let result = UseCaseResult::success(42);
        let r = result.as_ref();
        assert_eq!(*r.unwrap(), 42);
    }

    #[test]
    fn as_ref_failure() {
        let result: UseCaseResult<i32> = UseCaseResult::failure(UseCaseError::validation("V", "m"));
        let r = result.as_ref();
        assert!(r.is_failure());
    }

    #[test]
    fn and_then_chains_on_success_and_short_circuits_on_failure() {
        let ok = UseCaseResult::success(2).and_then(|v| UseCaseResult::success(v * 10));
        assert_eq!(ok.unwrap(), 20);

        let failed: UseCaseResult<i32> = UseCaseResult::failure(UseCaseError::not_found("NF", "x"));
        let chained = failed.and_then(|_| -> UseCaseResult<i32> { panic!("must not run") });
        assert_eq!(chained.unwrap_err().code(), "NF");
    }

    #[test]
    fn from_error_is_a_failure() {
        let r: UseCaseResult<()> = UseCaseError::validation("V", "m").into();
        assert!(r.is_failure());
        assert_eq!(r.as_result().unwrap_err().code(), "V");
    }

    #[test]
    fn debug_impl() {
        let s = UseCaseResult::success(42);
        let debug_str = format!("{:?}", s);
        assert!(debug_str.contains("Success"));
        assert!(debug_str.contains("42"));

        let f: UseCaseResult<i32> = UseCaseResult::failure(UseCaseError::validation("V", "msg"));
        let debug_str = format!("{:?}", f);
        assert!(debug_str.contains("Failure"));
    }
}
