//! A delivery target's terminal "do not retry" answer (Go
//! `common.IsDeliveryRejected`): HTTP 422 carrying
//! `FlowCatalyst-Outcome: reject`. Dispatch-job and scheduled-job delivery
//! fail the attempt's job at once instead of spending the remaining retries
//! (a function's `fn.Reject`; function-runner plan section 13.2). A plain 422
//! is still retried.

/// The response header that carries the outcome.
pub const DELIVERY_OUTCOME_HEADER: &str = "FlowCatalyst-Outcome";

/// The header value for a terminal rejection.
pub const DELIVERY_OUTCOME_REJECT: &str = "reject";

/// Whether a response is the terminal reject: status 422 and the outcome
/// header `reject` (surrounding whitespace and case ignored).
pub fn is_delivery_rejected(status: u16, outcome_header: Option<&str>) -> bool {
    status == 422
        && outcome_header.is_some_and(|v| v.trim().eq_ignore_ascii_case(DELIVERY_OUTCOME_REJECT))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_422_with_the_reject_outcome_is_terminal() {
        assert!(is_delivery_rejected(422, Some("reject")));
        assert!(is_delivery_rejected(422, Some("  REJECT ")));
        assert!(!is_delivery_rejected(422, None));
        assert!(!is_delivery_rejected(422, Some("retry")));
        assert!(!is_delivery_rejected(400, Some("reject")));
        assert!(!is_delivery_rejected(500, Some("reject")));
    }
}
