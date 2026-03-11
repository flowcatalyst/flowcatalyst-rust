//! Use Case Result Type
//!
//! Result type for use case execution. In the FlowCatalyst pattern, success
//! is typically created through `UnitOfWork::commit()` to ensure domain events
//! are always emitted. The SDK makes `success()` public so consumers can
//! implement custom `UnitOfWork` backends.

use super::error::UseCaseError;

/// Result type for use case execution.
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
pub enum UseCaseResult<T> {
    Success(T),
    Failure(UseCaseError),
}

impl<T> UseCaseResult<T> {
    /// Create a failure result.
    pub fn failure(error: UseCaseError) -> Self {
        UseCaseResult::Failure(error)
    }

    /// Create a success result.
    ///
    /// In the standard pattern, success is created by `UnitOfWork::commit()`.
    /// This is public in the SDK to allow custom `UnitOfWork` implementations.
    pub fn success(value: T) -> Self {
        UseCaseResult::Success(value)
    }

    pub fn is_success(&self) -> bool {
        matches!(self, UseCaseResult::Success(_))
    }

    pub fn is_failure(&self) -> bool {
        matches!(self, UseCaseResult::Failure(_))
    }

    pub fn unwrap(self) -> T {
        match self {
            UseCaseResult::Success(v) => v,
            UseCaseResult::Failure(e) => panic!("Called unwrap on a Failure: {}", e),
        }
    }

    pub fn unwrap_or(self, default: T) -> T {
        match self {
            UseCaseResult::Success(v) => v,
            UseCaseResult::Failure(_) => default,
        }
    }

    pub fn unwrap_or_else<F>(self, f: F) -> T
    where
        F: FnOnce(UseCaseError) -> T,
    {
        match self {
            UseCaseResult::Success(v) => v,
            UseCaseResult::Failure(e) => f(e),
        }
    }

    pub fn unwrap_err(self) -> UseCaseError {
        match self {
            UseCaseResult::Success(_) => panic!("Called unwrap_err on a Success"),
            UseCaseResult::Failure(e) => e,
        }
    }

    pub fn as_ref(&self) -> UseCaseResult<&T> {
        match self {
            UseCaseResult::Success(v) => UseCaseResult::Success(v),
            UseCaseResult::Failure(e) => UseCaseResult::Failure(e.clone()),
        }
    }

    pub fn map<U, F>(self, f: F) -> UseCaseResult<U>
    where
        F: FnOnce(T) -> U,
    {
        match self {
            UseCaseResult::Success(v) => UseCaseResult::Success(f(v)),
            UseCaseResult::Failure(e) => UseCaseResult::Failure(e),
        }
    }

    pub fn map_err<F>(self, f: F) -> UseCaseResult<T>
    where
        F: FnOnce(UseCaseError) -> UseCaseError,
    {
        match self {
            UseCaseResult::Success(v) => UseCaseResult::Success(v),
            UseCaseResult::Failure(e) => UseCaseResult::Failure(f(e)),
        }
    }

    pub fn into_result(self) -> Result<T, UseCaseError> {
        match self {
            UseCaseResult::Success(v) => Ok(v),
            UseCaseResult::Failure(e) => Err(e),
        }
    }
}

impl<T> From<UseCaseResult<T>> for Result<T, UseCaseError> {
    fn from(result: UseCaseResult<T>) -> Self {
        result.into_result()
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for UseCaseResult<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UseCaseResult::Success(v) => f.debug_tuple("Success").field(v).finish(),
            UseCaseResult::Failure(e) => f.debug_tuple("Failure").field(e).finish(),
        }
    }
}
