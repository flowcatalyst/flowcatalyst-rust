//! What a [`Context`](crate::Context) and a [`Request`](crate::Request) are
//! backed by: the real host (`crate::wasi`) or the test double
//! ([`crate::testing::TestHost`]).

use std::future::Future;
use std::pin::Pin;
use std::time::SystemTime;

use crate::context::Level;
#[cfg(feature = "flowcatalyst")]
use crate::context::{EmitError, Invocation};
use crate::http::{HttpCall, HttpError, HttpReply};
#[cfg(feature = "flowcatalyst")]
use fc_function_abi::OutboundEvent;

/// An outbound call in flight.
pub(crate) type HttpFuture = Pin<Box<dyn Future<Output = Result<HttpReply, HttpError>>>>;

pub(crate) trait Backend {
    /// The current invocation (read from the host once, on first use).
    #[cfg(feature = "flowcatalyst")]
    fn invocation(&self) -> &Invocation;

    #[cfg(feature = "flowcatalyst")]
    fn config(&self, key: &str) -> Option<String>;

    #[cfg(feature = "flowcatalyst")]
    fn secret(&self, key: &str) -> Option<String>;

    #[cfg(feature = "flowcatalyst")]
    fn emit(&self, event: &OutboundEvent) -> Result<(), EmitError>;

    fn log(&self, level: Level, message: &str);

    fn send(&self, call: HttpCall) -> HttpFuture;

    fn now(&self) -> SystemTime;
}
