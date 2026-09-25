//! Java `function/Runtime.java`.
//!
//! Java has exactly `jvm` and `wasm`, and so does this. The set is kept
//! extensible on purpose (owner ruling, 2026-09-24: the guest contract is
//! being decided separately): a new runtime is one line in the
//! `runtimes!` table below. What else a new runtime needs outside this
//! file: a migration widening `fn_functions_runtime_check`, and the
//! `runtime` enum of `function-manifest.schema.json` (which Java owns, so
//! that is a change made in Java first).

use crate::enum_str::str_enum;
use crate::ValidationError;

/// What a runtime's `entrypoint` must look like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntrypointRule {
    /// A binary class name: `^[A-Za-z_$][\w$]*(\.[A-Za-z_$][\w$]*)*$`.
    BinaryClassName,
    /// A WASM export name: `^[A-Za-z_]\w*$`.
    WasmExport,
}

impl EntrypointRule {
    /// Whether `raw` fits the rule (`\w` is ASCII, as in Java).
    pub fn matches(self, raw: &str) -> bool {
        let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
        match self {
            EntrypointRule::BinaryClassName => raw.split('.').all(|part| {
                let mut chars = part.chars();
                chars
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
                    && chars.all(|c| word(c) || c == '$')
            }),
            EntrypointRule::WasmExport => {
                let mut chars = raw.chars();
                chars
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                    && chars.all(word)
            }
        }
    }

    /// The `ENTRYPOINT_INVALID` message.
    pub fn invalid_message(self) -> &'static str {
        match self {
            EntrypointRule::BinaryClassName => "entrypoint must be a binary class name",
            EntrypointRule::WasmExport => "entrypoint must be a wasm export name",
        }
    }
}

/// Declares [`Runtime`]: one line per runtime, giving its variant, stored
/// spelling (the `fn_functions.runtime` column), manifest spelling, the
/// entrypoint rule, and whether `limits.wasmMemoryMb` applies to it.
macro_rules! runtimes {
    ($($variant:ident => $stored:literal, $wire:literal, $rule:ident, wasm_memory: $wasm:literal;)+) => {
        /// The runtime a function executes in. Stored upper-case in
        /// `fn_functions.runtime`; lower-case in the manifest's JSON.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Runtime {
            $($variant,)+
        }

        str_enum!(Runtime, "function runtime", { $($variant => $stored,)+ });

        impl Runtime {
            /// The lower-case spelling used in the manifest's JSON.
            pub fn wire_value(self) -> &'static str {
                match self {
                    $(Runtime::$variant => $wire,)+
                }
            }

            /// What this runtime's `entrypoint` must look like.
            pub fn entrypoint_rule(self) -> EntrypointRule {
                match self {
                    $(Runtime::$variant => EntrypointRule::$rule,)+
                }
            }

            /// Whether `limits.wasmMemoryMb` applies (`LIMIT_NOT_APPLICABLE`
            /// otherwise).
            pub fn takes_wasm_memory(self) -> bool {
                match self {
                    $(Runtime::$variant => $wasm,)+
                }
            }
        }
    };
}

runtimes! {
    Jvm => "JVM", "jvm", BinaryClassName, wasm_memory: false;
    Wasm => "WASM", "wasm", WasmExport, wasm_memory: true;
}

impl Runtime {
    /// `runtime is required and must be jvm or wasm`, listing every runtime.
    pub fn invalid_message() -> String {
        let names: Vec<&str> = Runtime::ALL.iter().map(|r| r.wire_value()).collect();
        let listed = match names.split_last() {
            Some((last, [])) => last.to_string(),
            Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
            None => String::new(),
        };
        format!("runtime is required and must be {listed}")
    }

    /// The manifest reader: case-insensitive; `None` for anything else.
    pub fn try_parse_strict(raw: &str) -> Option<Runtime> {
        let lower = raw.to_lowercase();
        Runtime::ALL
            .iter()
            .copied()
            .find(|r| r.wire_value() == lower)
    }

    /// `RUNTIME_INVALID` unless `raw` names a runtime, in any case.
    pub fn parse_strict(raw: &str) -> Result<Runtime, ValidationError> {
        Self::try_parse_strict(raw)
            .ok_or_else(|| ValidationError::new("RUNTIME_INVALID", Self::invalid_message()))
    }
}

/// Java `RuntimeTest`.
#[cfg(test)]
mod tests {
    use super::*;

    /// The Rust function host runs WASI components whose entrypoint is the
    /// `wasi:http/incoming-handler` export. Java's wasm entrypoint rule
    /// refuses `:` and `/`, so such a component is published under the alias
    /// `wasi_http_incoming_handler` (fc-fnhost-core `INCOMING_HANDLER_ALIAS`),
    /// which the unchanged rule must keep accepting.
    #[test]
    fn the_component_entrypoint_alias_is_a_valid_wasm_export() {
        assert!(EntrypointRule::WasmExport.matches("wasi_http_incoming_handler"));
        assert!(!EntrypointRule::WasmExport.matches("wasi:http/incoming-handler"));
    }

    #[test]
    fn stored_parse_is_exact() {
        assert_eq!("JVM".parse::<Runtime>().unwrap(), Runtime::Jvm);
        assert_eq!("WASM".parse::<Runtime>().unwrap(), Runtime::Wasm);
        assert!("jvm".parse::<Runtime>().is_err());
        assert_eq!(Runtime::Jvm.as_str(), "JVM");
    }

    #[test]
    fn parse_strict_is_case_insensitive() {
        assert_eq!(Runtime::parse_strict("jvm").unwrap(), Runtime::Jvm);
        assert_eq!(Runtime::parse_strict("Wasm").unwrap(), Runtime::Wasm);
        assert_eq!(Runtime::parse_strict("WASM").unwrap(), Runtime::Wasm);
    }

    #[test]
    fn parse_strict_rejects_unknown() {
        for raw in ["dotnet", "", " jvm"] {
            let err = Runtime::parse_strict(raw).unwrap_err();
            assert_eq!(err.code(), "RUNTIME_INVALID");
            assert_eq!(err.message(), "runtime is required and must be jvm or wasm");
        }
    }

    #[test]
    fn wire_value_is_lower_case() {
        assert_eq!(Runtime::Jvm.wire_value(), "jvm");
        assert_eq!(Runtime::Wasm.wire_value(), "wasm");
    }

    /// Exactly Java's set, until the owner adds one.
    #[test]
    fn exactly_javas_runtimes() {
        assert_eq!(Runtime::ALL, [Runtime::Jvm, Runtime::Wasm]);
        assert!(!Runtime::Jvm.takes_wasm_memory());
        assert!(Runtime::Wasm.takes_wasm_memory());
    }

    #[test]
    fn entrypoint_rules() {
        let class = EntrypointRule::BinaryClassName;
        for ok in [
            "x",
            "com.acme.billing.CreateInvoice",
            "$a.b_c$",
            "A1",
            "true",
        ] {
            assert!(class.matches(ok), "{ok}");
        }
        for bad in ["123bad", "a..b", ".a", "a.", "a-b", "é", "a b", ""] {
            assert!(!class.matches(bad), "{bad}");
        }
        let export = EntrypointRule::WasmExport;
        for ok in ["handle", "_start", "h1_x"] {
            assert!(export.matches(ok), "{ok}");
        }
        for bad in ["not a name", "1h", "a.b", "a$", "", "é"] {
            assert!(!export.matches(bad), "{bad}");
        }
    }
}
