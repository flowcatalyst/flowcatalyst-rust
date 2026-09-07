//! Queue backend selection by URI scheme.
//!
//! Mirrors Go's `queue.go:138-170` scheme resolution (`docs/spec/router.md`
//! §7.1): the backend key is the text before `://` (the whole string if
//! there is none), except an `http`/`https` URI whose host starts with
//! `sqs.` or `sqs-fips.` and contains `.amazonaws.` is treated as `sqs` —
//! that's what an SQS queue URL (`https://sqs.<region>.amazonaws.com/...`)
//! looks like. Anything else is an error naming the unrecognised scheme,
//! same as Go's `no consumer registered for scheme "<x>"`.
//!
//! This module has no backend-specific dependencies (no `sqs`/`postgres`/
//! `nats` cargo feature required) so it is always compiled and can be used
//! by a `ConsumerFactory` to decide which backend constructor to call
//! before touching any feature-gated code.

use crate::{QueueError, Result};

/// Which queue backend a `queueUri` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueScheme {
    Sqs,
    Postgres,
    Nats,
}

/// Resolve the backend a queue URI names, per the rules in the module doc.
///
/// `Err(QueueError::Config(..))` for a scheme with no registered backend —
/// the message is deliberately the same shape as Go's
/// `no consumer registered for scheme "<x>"` so operator-facing logs read
/// the same across implementations.
pub fn resolve_scheme(uri: &str) -> Result<QueueScheme> {
    let scheme = uri.split("://").next().unwrap_or(uri);
    let scheme_lower = scheme.to_ascii_lowercase();

    if scheme_lower == "http" || scheme_lower == "https" {
        let host = uri
            .splitn(2, "://")
            .nth(1)
            .unwrap_or("")
            .split(['/', '?', '#'])
            .next()
            .unwrap_or("")
            // strip a userinfo@ prefix and :port suffix, same as Go's
            // url.Parse(...).Hostname()
            .rsplit('@')
            .next()
            .unwrap_or("")
            .split(':')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();

        if (host.starts_with("sqs.") || host.starts_with("sqs-fips."))
            && host.contains(".amazonaws.")
        {
            return Ok(QueueScheme::Sqs);
        }

        return Err(QueueError::Config(format!(
            "no consumer registered for scheme \"{}\"",
            scheme
        )));
    }

    match scheme_lower.as_str() {
        "sqs" => Ok(QueueScheme::Sqs),
        "postgres" | "postgresql" => Ok(QueueScheme::Postgres),
        "nats" => Ok(QueueScheme::Nats),
        _ => Err(QueueError::Config(format!(
            "no consumer registered for scheme \"{}\"",
            scheme
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_sqs_scheme() {
        assert_eq!(
            resolve_scheme("sqs://queue-name").unwrap(),
            QueueScheme::Sqs
        );
    }

    #[test]
    fn postgres_scheme() {
        assert_eq!(
            resolve_scheme("postgres://user:pass@host:5432/db").unwrap(),
            QueueScheme::Postgres
        );
        assert_eq!(
            resolve_scheme("postgresql://user:pass@host:5432/db").unwrap(),
            QueueScheme::Postgres
        );
    }

    #[test]
    fn nats_scheme() {
        assert_eq!(
            resolve_scheme("nats://localhost:4222?stream=FLOWCATALYST").unwrap(),
            QueueScheme::Nats
        );
    }

    #[test]
    fn sqs_https_queue_url() {
        assert_eq!(
            resolve_scheme("https://sqs.eu-west-1.amazonaws.com/123456789012/my-queue.fifo")
                .unwrap(),
            QueueScheme::Sqs
        );
    }

    #[test]
    fn sqs_fips_https_queue_url() {
        assert_eq!(
            resolve_scheme("https://sqs-fips.us-east-1.amazonaws.com/123456789012/my-queue")
                .unwrap(),
            QueueScheme::Sqs
        );
    }

    #[test]
    fn sqs_http_localstack_style_is_not_amazonaws() {
        // LocalStack's dev-mode SQS host is sqs.<region>.localhost.localstack.cloud
        // — starts with "sqs." but does not contain ".amazonaws." — this
        // resolver correctly rejects it (dev mode wires SQS directly rather
        // than through scheme resolution, so this is deliberate, not a gap).
        assert!(resolve_scheme("http://sqs.eu-west-1.localhost.localstack.cloud:4566/000000000000/q")
            .is_err());
    }

    #[test]
    fn unknown_scheme_is_an_error() {
        let err = resolve_scheme("amqp://localhost").unwrap_err();
        assert!(err.to_string().contains("amqp"));
        assert!(err.to_string().contains("no consumer registered"));
    }

    #[test]
    fn scheme_is_case_insensitive() {
        assert_eq!(
            resolve_scheme("NATS://localhost:4222").unwrap(),
            QueueScheme::Nats
        );
        assert_eq!(
            resolve_scheme("POSTGRES://host/db").unwrap(),
            QueueScheme::Postgres
        );
    }
}
