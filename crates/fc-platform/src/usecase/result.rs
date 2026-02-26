//! Use Case Result Type
//!
//! A sealed result type for use case execution. Success can only be created
//! through the UnitOfWork, ensuring domain events are always emitted.

use super::error::UseCaseError;

/// Result type for use case execution.
///
/// This is similar to `Result<T, E>` but provides better ergonomics for
/// use case patterns and ensures that success can only be created through
/// the `UnitOfWork::commit()` method.
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
pub enum UseCaseResult<T> {
    /// Successful result containing the domain event.
    Success(T),
    /// Failed result containing the error.
    Failure(UseCaseError),
}

impl<T> UseCaseResult<T> {
    /// Create a failure result.
    ///
    /// This is public - any code can create failures for validation
    /// errors, business rule violations, etc.
    pub fn failure(error: UseCaseError) -> Self {
        UseCaseResult::Failure(error)
    }

    /// Create a success result.
    ///
    /// **Note:** In production code, success should only be created through
    /// `UnitOfWork::commit()` to ensure domain events are always emitted.
    /// This method is public to allow testing and internal use.
    pub(crate) fn success(value: T) -> Self {
        UseCaseResult::Success(value)
    }

    /// Check if this is a success result.
    pub fn is_success(&self) -> bool {
        matches!(self, UseCaseResult::Success(_))
    }

    /// Check if this is a failure result.
    pub fn is_failure(&self) -> bool {
        matches!(self, UseCaseResult::Failure(_))
    }

    /// Get the success value, consuming self.
    pub fn unwrap(self) -> T {
        match self {
            UseCaseResult::Success(v) => v,
            UseCaseResult::Failure(e) => panic!("Called unwrap on a Failure: {}", e),
        }
    }

    /// Get the success value or a default.
    pub fn unwrap_or(self, default: T) -> T {
        match self {
            UseCaseResult::Success(v) => v,
            UseCaseResult::Failure(_) => default,
        }
    }

    /// Get the success value or compute from closure.
    pub fn unwrap_or_else<F>(self, f: F) -> T
    where
        F: FnOnce(UseCaseError) -> T,
    {
        match self {
            UseCaseResult::Success(v) => v,
            UseCaseResult::Failure(e) => f(e),
        }
    }

    /// Get the error, consuming self.
    pub fn unwrap_err(self) -> UseCaseError {
        match self {
            UseCaseResult::Success(_) => panic!("Called unwrap_err on a Success"),
            UseCaseResult::Failure(e) => e,
        }
    }

    /// Get a reference to the success value.
    pub fn as_ref(&self) -> UseCaseResult<&T> {
        match self {
            UseCaseResult::Success(v) => UseCaseResult::Success(v),
            UseCaseResult::Failure(e) => UseCaseResult::Failure(e.clone()),
        }
    }

    /// Map the success value.
    pub fn map<U, F>(self, f: F) -> UseCaseResult<U>
    where
        F: FnOnce(T) -> U,
    {
        match self {
            UseCaseResult::Success(v) => UseCaseResult::Success(f(v)),
            UseCaseResult::Failure(e) => UseCaseResult::Failure(e),
        }
    }

    /// Map the error.
    pub fn map_err<F>(self, f: F) -> UseCaseResult<T>
    where
        F: FnOnce(UseCaseError) -> UseCaseError,
    {
        match self {
            UseCaseResult::Success(v) => UseCaseResult::Success(v),
            UseCaseResult::Failure(e) => UseCaseResult::Failure(f(e)),
        }
    }

    /// Convert to a standard Result.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_success_result() {
        let result: UseCaseResult<String> = UseCaseResult::success("test".to_string());
        assert!(result.is_success());
        assert!(!result.is_failure());
        assert_eq!(result.unwrap(), "test");
    }

    #[test]
    fn test_failure_result() {
        let result: UseCaseResult<String> =
            UseCaseResult::failure(UseCaseError::validation("CODE", "message"));
        assert!(!result.is_success());
        assert!(result.is_failure());
        assert_eq!(result.unwrap_err().code(), "CODE");
    }

    #[test]
    fn test_map() {
        let result: UseCaseResult<i32> = UseCaseResult::success(42);
        let mapped = result.map(|v| v * 2);
        assert_eq!(mapped.unwrap(), 84);
    }

    #[test]
    fn test_into_result() {
        let result: UseCaseResult<i32> = UseCaseResult::success(42);
        let std_result: Result<i32, UseCaseError> = result.into_result();
        assert_eq!(std_result.unwrap(), 42);
    }
}
