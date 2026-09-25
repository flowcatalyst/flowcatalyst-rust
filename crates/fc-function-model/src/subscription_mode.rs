//! A manifest subscription's `mode`: Java's `dispatch/DispatchMode`, read
//! strictly.

use crate::enum_str::str_enum;

/// How the platform dispatches a manifest subscription's events within a
/// message group. The same three modes as the router's `DispatchMode`
/// (`fc_common::DispatchMode`, which the platform converts this to), kept
/// here so the model does not depend on the router's crate; parsed
/// strictly, the exact constant name and nothing else, unlike the router's
/// lenient reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubscriptionMode {
    Immediate,
    NextOnError,
    BlockOnError,
}

str_enum!(SubscriptionMode, "dispatch mode", {
    Immediate => "IMMEDIATE",
    NextOnError => "NEXT_ON_ERROR",
    BlockOnError => "BLOCK_ON_ERROR",
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_is_the_exact_constant_name() {
        for mode in SubscriptionMode::ALL {
            assert_eq!(mode.as_str().parse::<SubscriptionMode>().unwrap(), *mode);
        }
        assert!("immediate".parse::<SubscriptionMode>().is_err());
        assert!("".parse::<SubscriptionMode>().is_err());
    }
}
