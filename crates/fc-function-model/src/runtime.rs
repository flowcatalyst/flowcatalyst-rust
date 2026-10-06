//! Java `function/Runtime.java`, plus `component` and `js`.
//!
//! Java has exactly `jvm` and `wasm`. Rust adds **`component`** (owner
//! decision 5, 2026-09-25): a WASI 0.2 component exporting
//! `wasi:http/incoming-handler`, said explicitly instead of left to the
//! host sniffing a `wasm` artifact. `wasm` keeps working exactly as before
//! (a Rust host still loads a component published under it, entrypoint
//! `wasi_http_incoming_handler`), and the two are compatible with each
//! other ([`Runtime::accepts_manifest`]).
//!
//! Rust also adds **`js`** (owner decisions 6 and 27): a single ES module
//! bundle, run in a V8 isolate by the Rust host's JS runtime
//! (`fc-fnhost-js`). Its `entrypoint` is the export that handles the
//! request, `default` unless the manifest names another, and
//! `limits.wasmMemoryMb` caps the isolate's memory (the V8 heap, and
//! separately its `ArrayBuffer` storage). `js` is compatible only with
//! itself.
//!
//! A new runtime is one line in the `runtimes!` table below, plus a
//! migration widening `fnr_functions_runtime_check` and the `runtime` enum
//! of `function-manifest.schema.json`.

use crate::enum_str::str_enum;
use crate::ValidationError;
use std::str;

/// What a runtime's `entrypoint` must look like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntrypointRule {
    /// A binary class name: `^[A-Za-z_$][\w$]*(\.[A-Za-z_$][\w$]*)*$`.
    BinaryClassName,
    /// A WASM export name: `^[A-Za-z_]\w*$`.
    WasmExport,
    /// The component's handler: `wasi:http/incoming-handler`, optionally
    /// `@0.2.<patch>`, or its manifest-safe alias
    /// [`INCOMING_HANDLER_ALIAS`].
    ComponentHandler,
    /// An ES module export name: `default`, or an identifier
    /// `^[A-Za-z_$][\w$]*$`.
    JsExport,
}

/// The one export a `component` function has, and its default
/// `entrypoint`.
pub const INCOMING_HANDLER: &str = "wasi:http/incoming-handler";

/// The manifest-safe name of [`INCOMING_HANDLER`], which Java's wasm rule
/// (`[A-Za-z_]\w*`) accepts: how a component is published as `runtime:
/// wasm`.
pub const INCOMING_HANDLER_ALIAS: &str = "wasi_http_incoming_handler";

/// A `js` function's default `entrypoint`: the bundle's default export.
pub const JS_DEFAULT_EXPORT: &str = "default";

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
            EntrypointRule::ComponentHandler => {
                raw == INCOMING_HANDLER_ALIAS
                    || match raw.strip_prefix(INCOMING_HANDLER) {
                        Some("") => true,
                        Some(version) => version.strip_prefix("@0.2.").is_some_and(|patch| {
                            !patch.is_empty() && patch.bytes().all(|b| b.is_ascii_digit())
                        }),
                        None => false,
                    }
            }
            EntrypointRule::JsExport => {
                let mut chars = raw.chars();
                chars
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
                    && chars.all(|c| word(c) || c == '$')
            }
        }
    }

    /// The `ENTRYPOINT_INVALID` message.
    pub fn invalid_message(self) -> &'static str {
        match self {
            EntrypointRule::BinaryClassName => "entrypoint must be a binary class name",
            EntrypointRule::WasmExport => "entrypoint must be a wasm export name",
            EntrypointRule::ComponentHandler => {
                "entrypoint must be wasi:http/incoming-handler (optionally @0.2.x) or wasi_http_incoming_handler"
            }
            EntrypointRule::JsExport => {
                "entrypoint must be the name of an ES module export (default, or an identifier)"
            }
        }
    }
}

/// What a WASM artifact is, from its first 8 bytes: the `\0asm` magic,
/// then a 16-bit version and a 16-bit layer (0: a core module, 1: a
/// component), both little-endian. A header check, not a parse: enough to
/// refuse a mismatched runtime at publish instead of at load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmKind {
    Component,
    CoreModule,
    /// Not WASM at all, or shorter than a header.
    NotWasm,
}

impl WasmKind {
    /// How many leading bytes [`WasmKind::sniff`] needs.
    pub const HEADER_LEN: usize = 8;

    pub fn sniff(bytes: &[u8]) -> WasmKind {
        match bytes {
            [0x00, 0x61, 0x73, 0x6d, _, _, 0x00, 0x00, ..] => WasmKind::CoreModule,
            [0x00, 0x61, 0x73, 0x6d, _, _, 0x01, 0x00, ..] => WasmKind::Component,
            _ => WasmKind::NotWasm,
        }
    }

    /// How a refusal names it.
    pub fn describe(self) -> &'static str {
        match self {
            WasmKind::Component => "a WASI component",
            WasmKind::CoreModule => "a core wasm module",
            WasmKind::NotWasm => "not wasm",
        }
    }
}

/// What an uploaded artifact looks like, from its first bytes: WASM (see
/// [`WasmKind`]), a jar, or text that can be a JavaScript bundle. A header
/// check, not a parse: the host compiles the bundle at load
/// (`JS_INVALID`); this only refuses the wrong kind of file at publish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    Wasm(WasmKind),
    /// A zip (`PK\x03\x04`): a jar.
    Jar,
    /// UTF-8 text with no NUL byte (a truncated trailing character is
    /// allowed: the prefix may end mid-character).
    Text,
    /// Anything else, or empty.
    Binary,
}

impl ArtifactKind {
    /// How many leading bytes [`ArtifactKind::sniff`] reads at most.
    pub const PREFIX_LEN: usize = 4096;

    /// Reads at most [`ArtifactKind::PREFIX_LEN`] bytes of `prefix`.
    pub fn sniff(prefix: &[u8]) -> ArtifactKind {
        let prefix = &prefix[..prefix.len().min(Self::PREFIX_LEN)];
        match WasmKind::sniff(prefix) {
            WasmKind::NotWasm => {}
            kind @ (WasmKind::Component | WasmKind::CoreModule) => return ArtifactKind::Wasm(kind),
        }
        if prefix.starts_with(b"PK\x03\x04") {
            return ArtifactKind::Jar;
        }
        if prefix.is_empty() || prefix.contains(&0) {
            return ArtifactKind::Binary;
        }
        match str::from_utf8(prefix) {
            Ok(_) => ArtifactKind::Text,
            // Only an incomplete character at the very end (`error_len`
            // None) is a cut, not an invalid byte.
            Err(e) if e.error_len().is_none() && prefix.len() == Self::PREFIX_LEN => {
                ArtifactKind::Text
            }
            Err(_) => ArtifactKind::Binary,
        }
    }

    /// How a refusal names it.
    pub fn describe(self) -> &'static str {
        match self {
            ArtifactKind::Wasm(kind) => kind.describe(),
            ArtifactKind::Jar => "a jar",
            ArtifactKind::Text => "text",
            ArtifactKind::Binary => "neither wasm nor UTF-8 text",
        }
    }
}

/// Declares [`Runtime`]: one line per runtime, giving its variant, stored
/// spelling (the `fnr_functions.runtime` column), manifest spelling, the
/// entrypoint rule, and whether `limits.wasmMemoryMb` applies to it.
macro_rules! runtimes {
    ($($variant:ident => $stored:literal, $wire:literal, $rule:ident, wasm_memory: $wasm:literal;)+) => {
        /// The runtime a function executes in. Stored upper-case in
        /// `fnr_functions.runtime`; lower-case in the manifest's JSON.
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
    Component => "COMPONENT", "component", ComponentHandler, wasm_memory: true;
    Js => "JS", "js", JsExport, wasm_memory: true;
}

impl Runtime {
    /// The `entrypoint` an absent one normalises to: a component's only
    /// export, or a JS bundle's default export; `jvm` and `wasm` require
    /// one.
    pub fn default_entrypoint(self) -> Option<&'static str> {
        match self {
            Runtime::Component => Some(INCOMING_HANDLER),
            Runtime::Js => Some(JS_DEFAULT_EXPORT),
            Runtime::Jvm | Runtime::Wasm => None,
        }
    }

    /// Whether a manifest saying `manifest` may be published to a function
    /// of this runtime (`RUNTIME_MISMATCH` otherwise): the same runtime, or
    /// `wasm` and `component` either way round. Both are WASM; `component`
    /// only says which kind, and the platform checks the artifact at
    /// publish.
    pub fn accepts_manifest(self, manifest: Runtime) -> bool {
        self == manifest
            || matches!(
                (self, manifest),
                (Runtime::Wasm, Runtime::Component) | (Runtime::Component, Runtime::Wasm)
            )
    }

    /// Whether an artifact of this runtime must be a WASI component (not a
    /// core module): `component` always. `wasm` may be either (Java's hosts
    /// run core modules), `jvm` is not WASM at all.
    pub fn requires_component(self) -> bool {
        self == Runtime::Component
    }

    /// Whether an artifact of this runtime must be a JavaScript bundle (a
    /// UTF-8 ES module, see [`ArtifactKind`]): `js` only.
    pub fn requires_js_bundle(self) -> bool {
        self == Runtime::Js
    }
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
    fn a_components_entrypoint_is_the_incoming_handler_or_its_alias() {
        let rule = Runtime::Component.entrypoint_rule();
        for ok in [
            "wasi:http/incoming-handler",
            "wasi:http/incoming-handler@0.2.0",
            "wasi:http/incoming-handler@0.2.12",
            "wasi_http_incoming_handler",
        ] {
            assert!(rule.matches(ok), "{ok}");
        }
        for bad in [
            "handle",
            "wasi:http/incoming-handler@0.3.0",
            "wasi:http/incoming-handler@0.2.",
            "wasi:http/incoming-handler@0.2.x",
            "wasi:http/outgoing-handler",
            "",
        ] {
            assert!(!rule.matches(bad), "{bad}");
        }
        assert_eq!(
            Runtime::Component.default_entrypoint(),
            Some("wasi:http/incoming-handler")
        );
        assert_eq!(Runtime::Wasm.default_entrypoint(), None);
        assert_eq!(Runtime::Jvm.default_entrypoint(), None);
    }

    #[test]
    fn a_wasm_header_says_component_or_core_module() {
        let core = [0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01];
        let component = [0x00, 0x61, 0x73, 0x6d, 0x0d, 0x00, 0x01, 0x00];
        assert_eq!(WasmKind::sniff(&core), WasmKind::CoreModule);
        assert_eq!(WasmKind::sniff(&component), WasmKind::Component);
        assert_eq!(WasmKind::sniff(b"PK\x03\x04 a jar"), WasmKind::NotWasm);
        assert_eq!(WasmKind::sniff(&component[..6]), WasmKind::NotWasm);
        assert_eq!(WasmKind::sniff(&[]), WasmKind::NotWasm);
    }

    #[test]
    fn wasm_and_component_accept_each_others_manifests() {
        assert!(Runtime::Wasm.accepts_manifest(Runtime::Component));
        assert!(Runtime::Component.accepts_manifest(Runtime::Wasm));
        assert!(Runtime::Component.accepts_manifest(Runtime::Component));
        assert!(!Runtime::Jvm.accepts_manifest(Runtime::Wasm));
        assert!(!Runtime::Component.accepts_manifest(Runtime::Jvm));
        assert!(Runtime::Component.requires_component());
        assert!(!Runtime::Wasm.requires_component());
    }

    #[test]
    fn a_js_function_takes_only_js_manifests_and_defaults_to_the_default_export() {
        assert!(Runtime::Js.accepts_manifest(Runtime::Js));
        for other in [Runtime::Jvm, Runtime::Wasm, Runtime::Component] {
            assert!(!Runtime::Js.accepts_manifest(other), "{other:?}");
            assert!(!other.accepts_manifest(Runtime::Js), "{other:?}");
        }
        assert_eq!(Runtime::Js.default_entrypoint(), Some("default"));
        assert!(Runtime::Js.requires_js_bundle());
        assert!(!Runtime::Js.requires_component());
        assert!(!Runtime::Component.requires_js_bundle());
        let rule = Runtime::Js.entrypoint_rule();
        for ok in ["default", "handle", "$h", "_x1"] {
            assert!(rule.matches(ok), "{ok}");
        }
        for bad in ["", "1x", "a.b", "a-b", "wasi:http/incoming-handler"] {
            assert!(!rule.matches(bad), "{bad}");
        }
    }

    #[test]
    fn an_artifact_is_sniffed_as_wasm_a_jar_text_or_binary() {
        let component = [0x00, 0x61, 0x73, 0x6d, 0x0d, 0x00, 0x01, 0x00];
        assert_eq!(
            ArtifactKind::sniff(&component),
            ArtifactKind::Wasm(WasmKind::Component)
        );
        assert_eq!(ArtifactKind::sniff(b"PK\x03\x04rest"), ArtifactKind::Jar);
        assert_eq!(
            ArtifactKind::sniff("export default () => new Response(\"é\");".as_bytes()),
            ArtifactKind::Text
        );
        assert_eq!(ArtifactKind::sniff(b""), ArtifactKind::Binary);
        assert_eq!(ArtifactKind::sniff(b"a\0b"), ArtifactKind::Binary);
        assert_eq!(
            ArtifactKind::sniff(&[0xff, 0xfe, 0x41]),
            ArtifactKind::Binary
        );
        // A prefix cut in the middle of a character is still text.
        let mut long = "a".repeat(ArtifactKind::PREFIX_LEN - 1).into_bytes();
        long.extend_from_slice("é".as_bytes());
        assert_eq!(ArtifactKind::sniff(&long), ArtifactKind::Text);
    }

    #[test]
    fn stored_parse_is_exact() {
        assert_eq!("JVM".parse::<Runtime>().unwrap(), Runtime::Jvm);
        assert_eq!("WASM".parse::<Runtime>().unwrap(), Runtime::Wasm);
        assert_eq!("COMPONENT".parse::<Runtime>().unwrap(), Runtime::Component);
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
            assert_eq!(
                err.message(),
                "runtime is required and must be jvm, wasm, component or js"
            );
        }
    }

    #[test]
    fn wire_value_is_lower_case() {
        assert_eq!(Runtime::Jvm.wire_value(), "jvm");
        assert_eq!(Runtime::Wasm.wire_value(), "wasm");
    }

    /// Java's set plus `component` (owner decision 5) and `js` (owner
    /// decisions 6 and 27).
    #[test]
    fn javas_runtimes_component_and_js() {
        assert_eq!(
            Runtime::ALL,
            [Runtime::Jvm, Runtime::Wasm, Runtime::Component, Runtime::Js]
        );
        assert!(Runtime::Js.takes_wasm_memory());
        assert_eq!(Runtime::Js.wire_value(), "js");
        assert_eq!(Runtime::Js.as_str(), "JS");
        assert_eq!("JS".parse::<Runtime>().unwrap(), Runtime::Js);
        assert_eq!(Runtime::parse_strict("JS").unwrap(), Runtime::Js);
        assert!(!Runtime::Jvm.takes_wasm_memory());
        assert!(Runtime::Wasm.takes_wasm_memory());
        assert!(Runtime::Component.takes_wasm_memory());
        assert_eq!(Runtime::Component.wire_value(), "component");
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
