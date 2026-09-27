//! The platform's permission match, for callers that only hold a token's
//! `scope` (Go `permissionMatches`, Java `Permission.grants`).

/// Whether any held permission grants `required`: an equal string, or the
/// same number of `:` segments with each held segment `*` or equal to the
/// required one. `platform:*:*:*` (super-admin) therefore grants every
/// four-segment platform permission.
pub fn grants<S: AsRef<str>>(held: &[S], required: &str) -> bool {
    held.iter().any(|p| matches(p.as_ref(), required))
}

/// One held permission against the required one (see [`grants`]).
pub fn matches(held: &str, required: &str) -> bool {
    if held == required {
        return true;
    }
    let h: Vec<&str> = held.split(':').collect();
    let r: Vec<&str> = required.split(':').collect();
    h.len() == r.len() && h.iter().zip(&r).all(|(h, r)| *h == "*" || h == r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_and_wildcard_segments() {
        let view = "platform:messaging:router:view";
        assert!(grants(&[view], view));
        assert!(grants(&["platform:*:*:*"], view));
        assert!(grants(&["platform:messaging:router:*"], view));
        assert!(grants(&["platform:messaging:*:view"], view));
        assert!(!grants(&["platform:messaging:router:operate"], view));
        // A different segment count never matches, wildcards or not.
        assert!(!grants(&["platform:*:*"], view));
        assert!(!grants(&["*"], view));
        assert!(!grants::<&str>(&[], view));
        // The wildcard is only on the held side.
        assert!(!grants(&[view], "platform:messaging:router:*"));
    }
}
