//! Java `function/FunctionAddress.java`: the type itself is
//! `fc_function_abi::FunctionAddress`, shared with the host and the guest
//! PDK. This adds the validation error for a malformed address, which the
//! platform returns as a 400 and the host reports as Java's `getMessage()`.

pub use fc_function_abi::{FunctionAddress, InvalidAddress};

use crate::ValidationError;

/// `ADDRESS_INVALID` (Java's message) unless `raw` is three DNS labels
/// separated by `.`.
pub fn parse_address(raw: &str) -> Result<FunctionAddress, ValidationError> {
    FunctionAddress::parse(raw).map_err(|e| ValidationError::new("ADDRESS_INVALID", e.to_string()))
}

/// Java `FunctionAddressTest` (the parse rules themselves are tested in
/// `fc-function-abi` against the shared `function-address-table.csv`).
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_address_is_a_validation_error() {
        for raw in ["billing.invoices", "a.b.c.d", "Billing.x.y", "a..b", ""] {
            let err = parse_address(raw).unwrap_err();
            assert_eq!(err.code(), "ADDRESS_INVALID");
            assert_eq!(
                err.message(),
                "address must be app.service.function: three DNS labels separated by '.'"
            );
        }
        assert_eq!(
            parse_address("").unwrap_err().to_string(),
            "validation: ADDRESS_INVALID: address must be app.service.function: three DNS labels separated by '.'"
        );
        assert_eq!(
            parse_address("billing.invoices.create").unwrap().render(),
            "billing.invoices.create"
        );
    }
}
