//! The model's two errors: a value that fails validation (Java's
//! `UseCaseException.validation(code, message)`), and a string that names no
//! variant of a string-backed enum.

use std::fmt;

/// A value or manifest that failed validation: Java's
/// `UseCaseException.validation(code, message)`. The platform maps it into
/// its own `UseCaseError` (a 400) with the same code and message; the
/// function host reports [`Display`](fmt::Display), which is Java's
/// `getMessage()`: `validation: CODE: message`.
///
/// `pointer` is set only on a [`ManifestProblem`](crate::ManifestProblem)'s
/// error, where the platform returns it as `details.pointer`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    code: &'static str,
    message: String,
    pointer: Option<String>,
}

impl ValidationError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            pointer: None,
        }
    }

    /// The same error, locating the problem in a manifest.
    pub fn with_pointer(mut self, pointer: impl Into<String>) -> Self {
        self.pointer = Some(pointer.into());
        self
    }

    /// The machine-readable code, e.g. `LABEL_INVALID`.
    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    /// The JSON pointer of a manifest problem (`""` is the document root).
    pub fn pointer(&self) -> Option<&str> {
        self.pointer.as_deref()
    }
}

/// Java's `UseCaseException.getMessage()`.
impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "validation: {}: {}", self.code, self.message)
    }
}

impl std::error::Error for ValidationError {}

/// A string that names no variant of the enum it was parsed as.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown {kind} {value:?} (expected one of: {})", expected.join(", "))]
pub struct UnknownEnumValue {
    /// What was being parsed, e.g. "client status".
    pub kind: &'static str,
    /// The rejected input.
    pub value: String,
    /// The canonical spellings that would have been accepted.
    pub expected: &'static [&'static str],
}

impl UnknownEnumValue {
    pub fn new(
        kind: &'static str,
        value: impl Into<String>,
        expected: &'static [&'static str],
    ) -> Self {
        Self {
            kind,
            value: value.into(),
            expected,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_is_javas_get_message() {
        let e = ValidationError::new("LABEL_INVALID", "bad label");
        assert_eq!(e.to_string(), "validation: LABEL_INVALID: bad label");
        assert_eq!(e.code(), "LABEL_INVALID");
        assert_eq!(e.message(), "bad label");
        assert_eq!(e.pointer(), None);
        assert_eq!(e.with_pointer("/pool").pointer(), Some("/pool"));
    }
}
