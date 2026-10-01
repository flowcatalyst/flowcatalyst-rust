//! [`ErrorChain`]: an error with every cause below it, for logs.
//!
//! Some clients put the useful part of a failure in its `source()` chain and
//! keep the top-level message short: reqwest says "error sending request for
//! url (…)" for a refused connection, a DNS failure and a netguard refusal
//! alike, and the AWS SDK says "service error" for every SQS error code. A
//! log line that prints only `%e` loses the reason.

use std::error::Error;
use std::fmt;

/// Displays an error followed by each `source()` below it, joined by `": "`.
/// A cause whose text its parent already ends with is skipped, so a wrapper
/// that prints its own cause does not repeat it.
pub struct ErrorChain<'a>(pub &'a (dyn Error + 'static));

impl fmt::Display for ErrorChain<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut last = self.0.to_string();
        f.write_str(&last)?;
        let mut source = self.0.source();
        while let Some(cause) = source {
            let text = cause.to_string();
            if !last.ends_with(&text) {
                write!(f, ": {text}")?;
            }
            last = text;
            source = cause.source();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::ErrorChain;
    use std::error::Error;
    use std::fmt;

    #[derive(Debug)]
    struct Layer {
        text: &'static str,
        cause: Option<Box<Layer>>,
    }

    impl fmt::Display for Layer {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.text)
        }
    }

    impl Error for Layer {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            self.cause.as_deref().map(|c| c as &(dyn Error + 'static))
        }
    }

    fn layer(text: &'static str, cause: Option<Layer>) -> Layer {
        Layer {
            text,
            cause: cause.map(Box::new),
        }
    }

    #[test]
    fn prints_every_cause() {
        let e = layer(
            "error sending request for url (http://staging-processor:8000/x)",
            Some(layer(
                "client error (Connect)",
                Some(layer(
                    "destination not allowed: 10.0.3.7 is a private address",
                    None,
                )),
            )),
        );
        assert_eq!(
            ErrorChain(&e).to_string(),
            "error sending request for url (http://staging-processor:8000/x): client error (Connect): destination not allowed: 10.0.3.7 is a private address"
        );
    }

    #[test]
    fn skips_a_cause_the_parent_already_printed() {
        let e = layer(
            "AWS SQS error: AccessDenied",
            Some(layer("AccessDenied", None)),
        );
        assert_eq!(ErrorChain(&e).to_string(), "AWS SQS error: AccessDenied");
    }

    #[test]
    fn a_lone_error_is_its_own_text() {
        assert_eq!(
            ErrorChain(&layer("timed out", None)).to_string(),
            "timed out"
        );
    }
}
