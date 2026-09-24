/// A value was rejected by a constructor, where Java throws
/// `IllegalArgumentException`. The message is Java's own.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct InvalidArgument(pub(crate) String);

impl InvalidArgument {
    /// The message, as Java words it.
    pub fn message(&self) -> &str {
        &self.0
    }
}
