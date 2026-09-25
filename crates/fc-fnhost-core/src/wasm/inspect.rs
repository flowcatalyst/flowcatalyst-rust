//! Load-time checks on an artifact, before anything is compiled (Java
//! `fnhost/wasm/WasmModuleCheck.java`, for components): what kind of wasm it
//! is, what it imports, whether it exports the manifest's entrypoint, and
//! whether its declared memory fits the function's cap. Every refusal is a
//! value naming its reason code, never a panic.
//!
//! The checks run in Java's order: imports, entrypoint, memory.

use wasmparser::{ComponentTypeRef, Parser, Payload};

/// A core module (the Extism / Java guest shape): refused, the host runs
/// components only.
pub const WASM_CORE_MODULE_UNSUPPORTED: &str = "WASM_CORE_MODULE_UNSUPPORTED";
/// Not wasm, unreadable, or does not compile.
pub const WASM_INVALID: &str = "WASM_INVALID";
/// The component imports something the host does not provide.
pub const WASM_IMPORT_NOT_ALLOWED: &str = "WASM_IMPORT_NOT_ALLOWED";
/// The manifest's entrypoint is not `wasi:http/incoming-handler`, or the
/// component does not export it.
pub const WASM_ENTRYPOINT_NOT_EXPORTED: &str = "WASM_ENTRYPOINT_NOT_EXPORTED";
/// A memory's declared minimum is over `limits.wasmMemoryMb`.
pub const WASM_MEMORY_OVER_CAP: &str = "WASM_MEMORY_OVER_CAP";

/// The one entrypoint a component function can have.
pub const INCOMING_HANDLER: &str = "wasi:http/incoming-handler";

/// A manifest-safe name for [`INCOMING_HANDLER`]. The platform's manifest
/// rule for a wasm `entrypoint` (Java's `WasmExport`, `[A-Za-z_]\w*`, mirrored
/// in fc-platform) rejects `:` and `/`, so a component published through the
/// unchanged management interface (Java's or Rust's) names its entrypoint
/// with this alias. It means exactly the unversioned incoming handler.
pub const INCOMING_HANDLER_ALIAS: &str = "wasi_http_incoming_handler";

/// A refusal: the heartbeat reports `LOAD:<reason>`; `detail` goes to the
/// host's log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub reason: &'static str,
    pub detail: String,
}

impl Refusal {
    pub fn new(reason: &'static str, detail: impl Into<String>) -> Self {
        Self {
            reason,
            detail: detail.into(),
        }
    }
}

/// The `wasi:*` interfaces the host links: the 0.2 `wasi:http/proxy` world
/// plus the rest of the 0.2 CLI world (which Rust's standard library, for
/// one, imports). Filesystem and sockets are linked but inert: there are no
/// preopened directories, and every socket use is denied.
const WASI_INTERFACES: &[(&str, &[&str])] = &[
    ("wasi:io", &["error", "poll", "streams"]),
    ("wasi:clocks", &["monotonic-clock", "wall-clock"]),
    ("wasi:random", &["random", "insecure", "insecure-seed"]),
    (
        "wasi:cli",
        &[
            "environment",
            "exit",
            "stdin",
            "stdout",
            "stderr",
            "terminal-input",
            "terminal-output",
            "terminal-stdin",
            "terminal-stdout",
            "terminal-stderr",
        ],
    ),
    ("wasi:filesystem", &["types", "preopens"]),
    (
        "wasi:sockets",
        &[
            "network",
            "instance-network",
            "udp",
            "udp-create-socket",
            "tcp",
            "tcp-create-socket",
            "ip-name-lookup",
        ],
    ),
    ("wasi:http", &["types", "outgoing-handler"]),
];

/// The host's own interfaces (`wit/flowcatalyst-function`).
const FLOWCATALYST_INTERFACES: &[&str] = &["config", "secrets", "log", "events", "invocation"];
const FLOWCATALYST_PACKAGE: &str = "flowcatalyst:function";

/// What an accepted component declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accepted {
    /// The top-level import names, in order.
    pub imports: Vec<String>,
    /// The largest declared maximum over the component's memories, in
    /// bytes, when every memory declares one.
    pub declared_max_memory: Option<u64>,
}

/// Checks `bytes` against the manifest's `entrypoint` and the memory cap.
pub fn check(bytes: &[u8], entrypoint: &str, cap_bytes: u64) -> Result<Accepted, Refusal> {
    if Parser::is_core_wasm(bytes) {
        return Err(Refusal::new(
            WASM_CORE_MODULE_UNSUPPORTED,
            "the artifact is a core wasm module; this host runs WASI 0.2 components that export wasi:http/incoming-handler",
        ));
    }
    if !Parser::is_component(bytes) {
        return Err(Refusal::new(WASM_INVALID, "not a wasm component"));
    }
    let shape = read(bytes)
        .map_err(|e| Refusal::new(WASM_INVALID, format!("not a wasm component: {e}")))?;

    for (name, is_instance) in &shape.imports {
        if !is_instance || !import_allowed(name) {
            return Err(Refusal::new(WASM_IMPORT_NOT_ALLOWED, name.clone()));
        }
    }

    let Some(wanted) = entrypoint_interface(entrypoint) else {
        return Err(Refusal::new(
            WASM_ENTRYPOINT_NOT_EXPORTED,
            format!(
                "the entrypoint must be {INCOMING_HANDLER} (optionally @0.2.x), not '{entrypoint}'"
            ),
        ));
    };
    if !shape.exports.iter().any(|e| export_matches(e, wanted)) {
        return Err(Refusal::new(
            WASM_ENTRYPOINT_NOT_EXPORTED,
            format!("the component does not export {entrypoint}"),
        ));
    }

    let mut declared_max = Some(0u64);
    for memory in &shape.memories {
        if memory.initial > cap_bytes {
            return Err(Refusal::new(
                WASM_MEMORY_OVER_CAP,
                format!(
                    "a memory declares {} MiB initially; the cap is {} MiB",
                    memory.initial >> 20,
                    cap_bytes >> 20
                ),
            ));
        }
        declared_max = match (declared_max, memory.maximum) {
            (Some(so_far), Some(max)) => Some(so_far.max(max)),
            _ => None,
        };
    }
    Ok(Accepted {
        imports: shape.imports.into_iter().map(|(name, _)| name).collect(),
        declared_max_memory: declared_max.filter(|_| !shape.memories.is_empty()),
    })
}

/// `wasi:http/incoming-handler` or `wasi:http/incoming-handler@0.2.x`
/// (the requested version, when given); `None` for anything else.
fn entrypoint_interface(entrypoint: &str) -> Option<Option<&str>> {
    let entrypoint = entrypoint.trim();
    match entrypoint.split_once('@') {
        None if entrypoint == INCOMING_HANDLER || entrypoint == INCOMING_HANDLER_ALIAS => {
            Some(None)
        }
        Some((name, version)) if name == INCOMING_HANDLER && is_02(version) => Some(Some(version)),
        _ => None,
    }
}

/// The component's export is the incoming handler at a 0.2.x version (the
/// manifest's exact version, when it names one).
fn export_matches(export: &str, wanted: Option<&str>) -> bool {
    match export.split_once('@') {
        Some((name, version)) if name == INCOMING_HANDLER && is_02(version) => {
            wanted.is_none_or(|w| w == version)
        }
        _ => false,
    }
}

fn is_02(version: &str) -> bool {
    version
        .strip_prefix("0.2.")
        .is_some_and(|rest| !rest.is_empty())
}

/// Whether a top-level instance import is one the host links.
pub fn import_allowed(name: &str) -> bool {
    let Some((qualified, version)) = name.split_once('@') else {
        return false;
    };
    let Some((package, interface)) = qualified.split_once('/') else {
        return false;
    };
    if package == FLOWCATALYST_PACKAGE {
        return version.starts_with("0.1.") && FLOWCATALYST_INTERFACES.contains(&interface);
    }
    is_02(version)
        && WASI_INTERFACES
            .iter()
            .any(|(p, interfaces)| *p == package && interfaces.contains(&interface))
}

struct Memory {
    initial: u64,
    maximum: Option<u64>,
}

struct Shape {
    /// `(name, is an instance import)`.
    imports: Vec<(String, bool)>,
    exports: Vec<String>,
    memories: Vec<Memory>,
}

/// The top-level component's imports and exports, and every memory any
/// nested core module defines.
fn read(bytes: &[u8]) -> wasmparser::Result<Shape> {
    let mut shape = Shape {
        imports: Vec::new(),
        exports: Vec::new(),
        memories: Vec::new(),
    };
    // `parse_all` walks nested modules and components inline, each one
    // closed by its own `End`; depth 0 is the outer component.
    let mut depth = 0usize;
    for payload in Parser::new(0).parse_all(bytes) {
        match payload? {
            Payload::ModuleSection { .. } | Payload::ComponentSection { .. } => depth += 1,
            Payload::End(_) => depth = depth.saturating_sub(1),
            Payload::ComponentImportSection(reader) if depth == 0 => {
                for import in reader {
                    let import = import?;
                    shape.imports.push((
                        import.name.name.to_owned(),
                        matches!(import.ty, ComponentTypeRef::Instance(_)),
                    ));
                }
            }
            Payload::ComponentExportSection(reader) if depth == 0 => {
                for export in reader {
                    shape.exports.push(export?.name.name.to_owned());
                }
            }
            Payload::MemorySection(reader) => {
                for memory in reader {
                    let memory = memory?;
                    let page = 1u64 << memory.page_size_log2.unwrap_or(16);
                    shape.memories.push(Memory {
                        initial: memory.initial.saturating_mul(page),
                        maximum: memory.maximum.map(|m| m.saturating_mul(page)),
                    });
                }
            }
            _ => {}
        }
    }
    Ok(shape)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entrypoint_names_the_incoming_handler_with_or_without_a_version() {
        assert_eq!(
            entrypoint_interface("wasi:http/incoming-handler"),
            Some(None)
        );
        assert_eq!(
            entrypoint_interface("wasi:http/incoming-handler@0.2.12"),
            Some(Some("0.2.12"))
        );
        // The manifest-safe alias the platform's entrypoint rule accepts.
        assert_eq!(
            entrypoint_interface("wasi_http_incoming_handler"),
            Some(None)
        );
        for bad in [
            "handle",
            "wasi:http/incoming-handler@0.3.0",
            "wasi:http/incoming-handler@",
            "wasi:http/outgoing-handler",
            "",
        ] {
            assert_eq!(entrypoint_interface(bad), None, "{bad}");
        }
        assert!(export_matches("wasi:http/incoming-handler@0.2.4", None));
        assert!(export_matches(
            "wasi:http/incoming-handler@0.2.4",
            Some("0.2.4")
        ));
        assert!(!export_matches(
            "wasi:http/incoming-handler@0.2.4",
            Some("0.2.12")
        ));
        assert!(!export_matches("wasi:http/incoming-handler", None));
    }

    #[test]
    fn only_the_proxy_and_cli_sets_and_our_own_package_are_allowed() {
        for ok in [
            "wasi:http/types@0.2.12",
            "wasi:http/outgoing-handler@0.2.0",
            "wasi:io/streams@0.2.4",
            "wasi:cli/environment@0.2.12",
            "wasi:sockets/tcp@0.2.0",
            "flowcatalyst:function/config@0.1.0",
            "flowcatalyst:function/events@0.1.3",
        ] {
            assert!(import_allowed(ok), "{ok}");
        }
        for bad in [
            "wasi:http/types",
            "wasi:http/types@0.3.0",
            "wasi:http/handler@0.2.12",
            "wasi:keyvalue/store@0.2.0",
            "flowcatalyst:function/config@0.2.0",
            "flowcatalyst:function/db@0.1.0",
            "evil:thing/iface@1.0.0",
            "wasi_snapshot_preview1",
        ] {
            assert!(!import_allowed(bad), "{bad}");
        }
    }
}
