//! Use Case Result Type
//!
//! A sealed result type for use case execution. Success can only be created
//! through the UnitOfWork, ensuring domain events are always emitted.

use super::error::UseCaseError;

/// Result type for use case execution.
///
/// A newtype over `Result<T, UseCaseError>` whose field is private, so a
/// success can only be constructed inside the `usecase` module — in
/// practice by `UnitOfWork::commit()` / `commit_delete()` / `emit_event()` /
/// `commit_all()`. Failures can be built anywhere with
/// [`UseCaseResult::failure`]. Callers consume it with
/// [`UseCaseResult::into_result`].
///
/// # Usage
///
/// ```ignore
/// // Return failure for validation/business rule violations
/// if !is_valid {
///     return UseCaseResult::failure(UseCaseError::validation("INVALID", "Invalid input"));
/// }
///
/// // Return success only through UnitOfWork.commit()
/// unit_of_work.commit(aggregate, event, command).await
/// ```
///
/// # Using `?` in a use case
///
/// `UseCaseResult` does not implement `Try`, so `execute` cannot use `?`
/// directly. Put the loading and rule checks in an inherent
/// `async fn prepare(..) -> Result<_, UseCaseError>`, where `?` works
/// (repository errors convert via `From<PlatformError>`, and
/// [`OrNotFound::or_not_found`](super::OrNotFound) turns `Ok(None)` into a
/// 404), and keep only the hand-off to the UnitOfWork in `execute`:
///
/// ```ignore
/// async fn execute(&self, command: Cmd, ctx: ExecutionContext) -> UseCaseResult<Evt> {
///     let (role, event) = match self.prepare(&command, &ctx).await {
///         Ok(v) => v,
///         Err(e) => return UseCaseResult::failure(e),
///     };
///     self.unit_of_work.commit(&role, &*self.role_repo, event, &command).await
/// }
/// ```
///
/// # The seal
///
/// Code outside the `usecase` module cannot fabricate a success, either
/// through the constructor:
///
/// ```compile_fail
/// use fc_platform::usecase::UseCaseResult;
/// let _: UseCaseResult<u32> = UseCaseResult::success(1);
/// ```
///
/// or through the wrapped `Result`:
///
/// ```compile_fail
/// use fc_platform::usecase::UseCaseResult;
/// let _: UseCaseResult<u32> = UseCaseResult(Ok(1));
/// ```
///
/// whereas building a failure is allowed:
///
/// ```
/// use fc_platform::usecase::{UseCaseError, UseCaseResult};
/// let r: UseCaseResult<u32> = UseCaseResult::failure(UseCaseError::validation("CODE", "msg"));
/// assert!(r.into_result().is_err());
/// ```
#[must_use]
pub struct UseCaseResult<T>(Result<T, UseCaseError>);

impl<T> UseCaseResult<T> {
    /// Create a failure result.
    ///
    /// This is public - any code can create failures for validation
    /// errors, business rule violations, etc.
    pub fn failure(error: UseCaseError) -> Self {
        Self(Err(error))
    }

    /// Create a success result.
    ///
    /// Visible only within the `usecase` module so that only `UnitOfWork`
    /// (and its associated helpers) can construct a success. Use cases
    /// defined outside this module — i.e. every `*UseCase::execute` —
    /// must route through `unit_of_work.commit()` / `commit_delete()` /
    /// `emit_event()` to return success.
    pub(in crate::usecase) fn success(value: T) -> Self {
        Self(Ok(value))
    }

    /// Borrow the outcome as a standard `Result`.
    pub fn as_result(&self) -> Result<&T, &UseCaseError> {
        self.0.as_ref()
    }

    /// Map the success value.
    pub fn map<U, F>(self, f: F) -> UseCaseResult<U>
    where
        F: FnOnce(T) -> U,
    {
        UseCaseResult(self.0.map(f))
    }

    /// Convert to a standard Result.
    pub fn into_result(self) -> Result<T, UseCaseError> {
        self.0
    }
}

impl<T> From<UseCaseResult<T>> for Result<T, UseCaseError> {
    fn from(result: UseCaseResult<T>) -> Self {
        result.0
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
    fn test_success_result() {
        let result: UseCaseResult<String> = UseCaseResult::success("test".to_string());
        assert!(result.as_result().is_ok());
        assert_eq!(result.into_result().unwrap(), "test");
    }

    #[test]
    fn test_failure_result() {
        let result: UseCaseResult<String> =
            UseCaseResult::failure(UseCaseError::validation("CODE", "message"));
        assert!(result.as_result().is_err());
        assert_eq!(result.into_result().unwrap_err().code(), "CODE");
    }

    #[test]
    fn test_map() {
        let result: UseCaseResult<i32> = UseCaseResult::success(42);
        let mapped = result.map(|v| v * 2);
        assert_eq!(mapped.into_result().unwrap(), 84);
    }

    #[test]
    fn test_into_result() {
        let result: UseCaseResult<i32> = UseCaseResult::success(42);
        let std_result: Result<i32, UseCaseError> = result.into_result();
        assert_eq!(std_result.unwrap(), 42);
    }
}
