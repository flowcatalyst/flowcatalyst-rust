//! Small helpers that reproduce JDK / Jackson behaviour the Java host relies
//! on, so parsing decisions land the same way on both hosts.

// `String.isBlank`, `JsonNode.asString`, node iteration and `canConvertToInt`
// are the function model's, shared with the platform and the verifier.
pub(crate) use fc_function_model::java::{as_java_int, as_string, elements, is_blank};

/// `String.length()`, in UTF-16 code units.
pub(crate) fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}
