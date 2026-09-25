use std::fmt;

use fc_function_abi::Response;

/// Any error a handler can return with `?`: every `std::error::Error`
/// converts into it (like `anyhow::Error`, which a handler may use instead).
///
/// An `Err` from a handler goes to the function's log at ERROR (its `{:#}`
/// form: the error and its causes) and answers the host's generic
/// `500 {"error":"the function failed"}`, so internals never reach a caller.
/// To tell the caller something, return `Response::fail(message)`.
pub struct Error(Box<dyn std::error::Error + Send + Sync + 'static>);

/// `Result<T, fc_function_pdk::Error>`.
pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    /// An error that is only a message.
    pub fn msg(message: impl fmt::Display) -> Self {
        Self(Box::new(Message(message.to_string())))
    }

    /// The underlying error.
    pub fn inner(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
        &*self.0
    }

    /// Whether the underlying error is an `E`.
    pub fn is<E: std::error::Error + 'static>(&self) -> bool {
        self.0.is::<E>()
    }

    /// The underlying error, when it is an `E`.
    pub fn downcast_ref<E: std::error::Error + 'static>(&self) -> Option<&E> {
        self.0.downcast_ref::<E>()
    }
}

impl<E: std::error::Error + Send + Sync + 'static> From<E> for Error {
    fn from(error: E) -> Self {
        Self(Box::new(error))
    }
}

/// The message; `{:#}` adds each cause, `: `-separated.
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)?;
        if f.alternate() {
            let mut source = self.0.source();
            while let Some(cause) = source {
                write!(f, ": {cause}")?;
                source = cause.source();
            }
        }
        Ok(())
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

#[derive(Debug)]
struct Message(String);

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Message {}

/// What a `#[handler]` function may return: a [`Response`], or a
/// `Result<Response, E>` whose error is displayable ([`Error`],
/// `anyhow::Error`, any `std::error::Error`).
pub trait HandlerOutput {
    /// The response, or the failure's message (the `{:#}` form when the
    /// error supports it: the message and its causes).
    fn into_response(self) -> std::result::Result<Response, String>;
}

impl HandlerOutput for Response {
    fn into_response(self) -> std::result::Result<Response, String> {
        Ok(self)
    }
}

impl<E: fmt::Display> HandlerOutput for std::result::Result<Response, E> {
    fn into_response(self) -> std::result::Result<Response, String> {
        self.map_err(|e| format!("{e:#}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Outer(std::io::Error);
    impl fmt::Display for Outer {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("reading the order")
        }
    }
    impl std::error::Error for Outer {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&self.0)
        }
    }

    #[test]
    fn display_is_the_message_and_alternate_adds_the_causes() {
        let e = Error::from(Outer(std::io::Error::other("disk gone")));
        assert_eq!(e.to_string(), "reading the order");
        assert_eq!(format!("{e:#}"), "reading the order: disk gone");
        assert!(e.is::<Outer>());
        assert!(e.downcast_ref::<std::io::Error>().is_none());
    }

    #[test]
    fn an_err_carries_its_causes_for_the_log() {
        let out: Result<Response> = Err(Error::from(Outer(std::io::Error::other("disk gone"))));
        assert_eq!(
            out.into_response().unwrap_err(),
            "reading the order: disk gone"
        );
    }

    #[test]
    fn a_response_and_an_ok_pass_through() {
        assert_eq!(Response::ack().into_response(), Ok(Response::ack()));
        let ok: std::result::Result<Response, std::fmt::Error> = Ok(Response::ack());
        assert_eq!(ok.into_response(), Ok(Response::ack()));
    }
}
