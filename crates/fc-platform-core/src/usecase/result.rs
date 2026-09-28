//! Use Case Result Types
//!
//! [`Committed`] is the sealed proof that a unit of work committed: only the
//! `usecase` module can build one, so a use case's `execute` can only
//! succeed by handing its write to the UnitOfWork. [`UseCaseResult`] is
//! what [`UseCase::run`](super::UseCase::run) hands to handlers.

use super::error::UseCaseError;

/// A value a unit of work has committed.
///
/// Its constructor is visible only inside the `usecase` module, so the only
/// ways to get one are `UnitOfWork::commit()` / `commit_delete()` /
/// `emit_event()` / `emit_events()` / `commit_all()` /
/// `commit_all_with_events()` / `commit_sync()` and `PgUnitOfWork::run()`. `UseCase::execute`
/// returns `Result<Committed<Event>, UseCaseError>`, so `?` works in it and
/// its happy path can only end in one of those calls:
///
/// ```ignore
/// async fn execute(&self, command: Cmd, ctx: ExecutionContext)
///     -> Result<Committed<RoleUpdated>, UseCaseError>
/// {
///     let mut role = self.role_repo.find_by_id(&command.id).await
///         .or_not_found("ROLE_NOT_FOUND", "Role not found")?;
///     if role.is_built_in() {
///         return Err(UseCaseError::business_rule("BUILT_IN", "..."));
///     }
///     role.rename(&command.name);
///     let event = RoleUpdated::new(&ctx, &role);
///     self.unit_of_work.commit(&role, &*self.role_repo, event, &command).await
/// }
/// ```
///
/// # The seal
///
/// Code outside the `usecase` module cannot fabricate one, either through a
/// constructor:
///
/// ```compile_fail
/// use fc_platform_core::usecase::Committed;
/// let _: Committed<u32> = Committed::new(1);
/// ```
///
/// or through the tuple field:
///
/// ```compile_fail
/// use fc_platform_core::usecase::Committed;
/// let _: Committed<u32> = Committed(1);
/// ```
///
/// Mapping a committed value cannot forge one, so [`Committed::map`],
/// [`Committed::into_inner`] and `as_ref` (via [`AsRef`]) are public.
pub struct Committed<T>(T);

impl<T> Committed<T> {
    /// Seal a committed value. Only the unit of work calls this.
    pub(in crate::usecase) fn new(value: T) -> Self {
        Self(value)
    }

    /// The committed value.
    pub fn into_inner(self) -> T {
        self.0
    }

    /// Map the committed value.
    pub fn map<U, F>(self, f: F) -> Committed<U>
    where
        F: FnOnce(T) -> U,
    {
        Committed(f(self.0))
    }
}

/// Borrow the committed value (`committed.as_ref()`).
impl<T> AsRef<T> for Committed<T> {
    fn as_ref(&self) -> &T {
        &self.0
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Committed<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Committed").field(&self.0).finish()
    }
}

/// The outcome of [`UseCase::run`](super::UseCase::run).
///
/// A newtype over `Result<T, UseCaseError>` whose field is private: a
/// success comes only from a [`Committed`] value (via `From`), so it too
/// can only come out of a unit of work. Failures can be built anywhere
/// with [`UseCaseResult::failure`]. Callers consume it with
/// [`UseCaseResult::into_result`].
///
/// Code outside the `usecase` module cannot fabricate a success through the
/// wrapped `Result`:
///
/// ```compile_fail
/// use fc_platform_core::usecase::UseCaseResult;
/// let _: UseCaseResult<u32> = UseCaseResult(Ok(1));
/// ```
///
/// whereas building a failure is allowed:
///
/// ```
/// use fc_platform_core::usecase::{UseCaseError, UseCaseResult};
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

    /// The outcome with its success still sealed: what a
    /// [`PgUnitOfWork::run`](super::PgUnitOfWork::run) closure returns
    /// after running its last use case.
    pub fn into_committed(self) -> Result<Committed<T>, UseCaseError> {
        self.0.map(Committed)
    }
}

impl<T> From<Result<Committed<T>, UseCaseError>> for UseCaseResult<T> {
    fn from(result: Result<Committed<T>, UseCaseError>) -> Self {
        Self(result.map(Committed::into_inner))
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

    fn success<T>(value: T) -> UseCaseResult<T> {
        UseCaseResult::from(Ok(Committed::new(value)))
    }

    #[test]
    fn test_success_result() {
        let result: UseCaseResult<String> = success("test".to_string());
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
        let mapped = success(42).map(|v| v * 2);
        assert_eq!(mapped.into_result().unwrap(), 84);
    }

    #[test]
    fn test_into_result() {
        let std_result: Result<i32, UseCaseError> = success(42).into_result();
        assert_eq!(std_result.unwrap(), 42);
    }

    #[test]
    fn committed_maps_and_unwraps() {
        let committed = Committed::new(21).map(|v| v * 2);
        let borrowed: &i32 = committed.as_ref();
        assert_eq!(*borrowed, 42);
        assert_eq!(committed.into_inner(), 42);
    }

    #[test]
    fn into_committed_keeps_the_outcome() {
        assert_eq!(success(7).into_committed().unwrap().into_inner(), 7);
        let err = UseCaseResult::<u8>::failure(UseCaseError::validation("CODE", "m"))
            .into_committed()
            .unwrap_err();
        assert_eq!(err.code(), "CODE");
    }
}
