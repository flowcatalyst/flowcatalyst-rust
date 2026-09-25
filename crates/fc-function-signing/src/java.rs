//! Small helpers that reproduce JDK / Jackson behaviour Java's verifier
//! relies on, so parsing decisions land the same way in Rust.

use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use base64::Engine;

/// `java.util.Base64.getDecoder()`: the standard alphabet, padding optional,
/// no whitespace, lenient about trailing bits.
pub(crate) const JAVA_BASE64: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true),
);

pub(crate) fn b64_decode(text: &str) -> Result<Vec<u8>, base64::DecodeError> {
    JAVA_BASE64.decode(text)
}

// `String.isBlank`, `JsonNode.asString` and node iteration are the function
// model's, shared with the platform and the host.
pub(crate) use fc_function_model::java::{as_string, elements, is_blank};

/// `new String(bytes, UTF_8)`: malformed sequences become U+FFFD.
pub(crate) fn utf8_lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_padding_is_optional() {
        assert_eq!(b64_decode("YQ").unwrap(), b"a");
        assert_eq!(b64_decode("YQ==").unwrap(), b"a");
        assert!(b64_decode("Y Q==").is_err());
    }
}
