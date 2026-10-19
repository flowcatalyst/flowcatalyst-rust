//! What an operator requeue may touch, by status (Go
//! `dispatchjob/requeue_rules.go`). One statement of the rule for the id-list
//! requeue, requeue-by-filter and the preview; the lifecycle enforces it again
//! in SQL (`REQUEUE` / `REDRIVE`), so a job that changed status after it was
//! read is refused rather than reset.
//!
//! - PENDING, FAILED (and its legacy alias ERROR), CANCELLED, EXPIRED: requeued.
//!   PENDING is reset too: requeueing a job sitting out a retry backoff makes
//!   it due now.
//! - QUEUED and PROCESSING (and the legacy alias IN_PROGRESS): never. A
//!   delivery is under way or on the queue; resetting it risks a double
//!   delivery.
//! - COMPLETED: only as an explicit, doubly confirmed redrive.

/// The statuses a requeue resets, as stored (the legacy `ERROR` spelling of
/// FAILED included).
pub const REQUEUEABLE_STATUSES: [&str; 5] = ["PENDING", "FAILED", "ERROR", "CANCELLED", "EXPIRED"];

/// The literal a request sends as `confirmRedriveCompleted`, next to
/// `redriveCompleted: true`, to redrive COMPLETED jobs.
pub const REDRIVE_CONFIRMATION: &str = "REDRIVE";

/// Skip reason: QUEUED or PROCESSING, never reset.
pub const SKIP_IN_FLIGHT: &str = "IN_FLIGHT";
/// Skip reason: a COMPLETED job without a confirmed redrive.
pub const SKIP_COMPLETED_NOT_CONFIRMED: &str = "COMPLETED_REDRIVE_NOT_CONFIRMED";

/// What a requeue does with a job of a given status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Reset to PENDING.
    Requeue,
    /// COMPLETED; reset only for a confirmed redrive.
    Completed,
    /// QUEUED or PROCESSING; never reset.
    InFlight,
}

/// Go `ClassifyForRequeue`: the rule above applied to a stored status. A
/// status the rule does not name is `Requeue`; the SQL guard still refuses it.
pub fn classify_for_requeue(status: &str) -> Verdict {
    match status {
        "COMPLETED" => Verdict::Completed,
        "QUEUED" | "PROCESSING" | "IN_PROGRESS" => Verdict::InFlight,
        _ => Verdict::Requeue,
    }
}

/// Go `RedriveConfirmed`: whether a request's two redrive fields together
/// authorise redriving COMPLETED jobs.
pub fn redrive_confirmed(redrive_completed: bool, confirm: &str) -> bool {
    redrive_completed && confirm == REDRIVE_CONFIRMATION
}

/// The statuses the requeue-by-filter id walk hands to the requeue: what a
/// requeue may reset, plus COMPLETED for a confirmed redrive (Go
/// `resettableStatuses`).
pub fn resettable_statuses(redrive: bool) -> Vec<String> {
    REQUEUEABLE_STATUSES
        .iter()
        .copied()
        .chain(redrive.then_some("COMPLETED"))
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_status_has_the_verdict_go_gives_it() {
        for s in [
            "PENDING",
            "FAILED",
            "ERROR",
            "CANCELLED",
            "EXPIRED",
            "SOMETHING",
        ] {
            assert_eq!(classify_for_requeue(s), Verdict::Requeue, "{s}");
        }
        for s in ["QUEUED", "PROCESSING", "IN_PROGRESS"] {
            assert_eq!(classify_for_requeue(s), Verdict::InFlight, "{s}");
        }
        assert_eq!(classify_for_requeue("COMPLETED"), Verdict::Completed);
    }

    #[test]
    fn a_redrive_needs_both_fields() {
        assert!(redrive_confirmed(true, "REDRIVE"));
        assert!(!redrive_confirmed(true, "redrive"));
        assert!(!redrive_confirmed(true, ""));
        assert!(!redrive_confirmed(false, "REDRIVE"));
    }

    #[test]
    fn the_walk_resets_what_a_requeue_may_and_completed_only_for_a_redrive() {
        assert_eq!(
            resettable_statuses(false),
            ["PENDING", "FAILED", "ERROR", "CANCELLED", "EXPIRED"]
        );
        assert_eq!(
            resettable_statuses(true).last().map(String::as_str),
            Some("COMPLETED")
        );
        assert_eq!(resettable_statuses(true).len(), 6);
    }
}
