//! Convention test: where routes are wired.
//!
//! Every route module exposes one entry point,
//! `pub fn routes(ctx: &PlatformContext) -> AggregateRoutes`, in its
//! `routes.rs`; it builds its own state from the context and returns its
//! routes at their full paths. `router.rs` only lists those entry points and
//! adds the cross-cutting layers. Four rules keep it that way:
//!
//! 1. **`router.rs` imports no handler or state type.** Its `use`s are axum,
//!    Swagger UI, std and the context types; it names no `*State` and calls
//!    no `*_router(..)` other than the cross-cutting ones in
//!    [`ROUTER_RS_CALLS`].
//! 2. **Routes are registered only in `routes.rs`.** `.route(`, `.routes(`,
//!    `.nest(`, `.nest_service(` and `.fallback(` appear in no other file of
//!    `src/` (test modules aside), except the cross-cutting files in
//!    [`REGISTRATION_ALLOWLIST`], each with its reason.
//! 3. **Every `routes.rs` has the entry point** — `pub fn routes(ctx:
//!    &PlatformContext) -> AggregateRoutes` — **and `router.rs` mounts every
//!    one**, so a module's routes can neither be forgotten nor mounted twice.
//! 4. **`router.rs` mounts nothing else**: every `.merge(` in its `build`
//!    takes a module's `routes(ctx)`, the developer portal (which needs the
//!    finished document), or a cross-cutting router.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// Files other than `routes.rs` and `router.rs` that may register routes.
const REGISTRATION_ALLOWLIST: &[(&str, &str)] = &[
    (
        "shared/openapi_api.rs",
        "the published OpenAPI document as JSON and YAML (Go's wire_spec.go); \
         cross-cutting, mounted by router.rs once the document exists",
    ),
    (
        "shared/openapi_contract.rs",
        "utoipa `OpenApi::nest` of documentation for Go's contract, not routing",
    ),
];

/// Calls `router.rs` may make besides the modules' `routes(ctx)`.
const ROUTER_RS_CALLS: &[(&str, &str)] = &[
    (
        "crate::shared::openapi_api::openapi_router",
        "the published document's JSON/YAML routes, built from the document",
    ),
    (
        "crate::shared::routes::developer_portal_routes",
        "the developer portal serves the platform's own document, so it is \
         built after the other routes",
    ),
];

/// `use` paths `router.rs` may import.
const ROUTER_RS_USES: &[&str] = &[
    "axum",
    "utoipa_swagger_ui",
    "std",
    "crate::shared::platform_context",
];

fn rel(p: &Path) -> String {
    crate::support::sources::strip_src(p)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/")
}

/// Index after the bracket matching the one at `open`, skipping string
/// literals and comments.
fn matching(s: &[u8], open: usize) -> usize {
    let (o, c) = match s[open] {
        b'{' => (b'{', b'}'),
        b'(' => (b'(', b')'),
        _ => panic!("not a bracket"),
    };
    let mut depth = 0i32;
    let mut i = open;
    while i < s.len() {
        match s[i] {
            b'/' if s.get(i + 1) == Some(&b'/') => {
                while i < s.len() && s[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'"' => {
                i += 1;
                while i < s.len() && s[i] != b'"' {
                    if s[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b if b == o => depth += 1,
            b if b == c => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    s.len()
}

/// The source without its `#[cfg(test)]` modules and without comments.
fn production_code(content: &str) -> String {
    let mut out = content.to_string();
    while let Some(at) = out.find("#[cfg(test)]\nmod ") {
        let open = at + out[at..].find('{').unwrap();
        let end = matching(out.as_bytes(), open);
        out.replace_range(at..end, "");
    }
    out.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The body of `fn name` in `content`.
fn fn_body<'a>(content: &'a str, name: &str) -> &'a str {
    let re = regex::Regex::new(&format!(r"(?m)^\s*pub fn {name}\b")).unwrap();
    let m = re.find(content).unwrap_or_else(|| panic!("no fn {name}"));
    let open = m.end() + content[m.end()..].find('{').unwrap();
    let end = matching(content.as_bytes(), open);
    &content[open..end]
}

#[test]
fn router_rs_imports_no_handler_or_state_type() {
    let content = fs::read_to_string(crate::support::sources::file("router.rs")).unwrap();
    let code = production_code(&content);
    let mut problems = Vec::new();

    let use_re = regex::Regex::new(r"(?m)^use\s+([A-Za-z_][A-Za-z0-9_:]*)").unwrap();
    for c in use_re.captures_iter(&code) {
        let path = &c[1];
        if !ROUTER_RS_USES
            .iter()
            .any(|allowed| path == *allowed || path.starts_with(&format!("{allowed}::")))
        {
            problems.push(format!("imports `{path}`"));
        }
    }
    let state_re = regex::Regex::new(r"\b([A-Z][A-Za-z0-9]*State)\b").unwrap();
    for c in state_re.captures_iter(&code) {
        problems.push(format!("names the state type `{}`", &c[1]));
    }
    let handler_mod_re =
        regex::Regex::new(r"crate::[a-z_]+::(api|bff|[a-z_]+_api)::([a-z_]+)").unwrap();
    for c in handler_mod_re.captures_iter(&code) {
        let full = c.get(0).unwrap().as_str();
        if !ROUTER_RS_CALLS.iter().any(|(call, _)| *call == full) {
            problems.push(format!("reaches into a handler module: `{full}`"));
        }
    }
    let router_call_re =
        regex::Regex::new(r"((?:[A-Za-z_][A-Za-z0-9_]*::)*[A-Za-z0-9_]*_router)\s*\(").unwrap();
    for c in router_call_re.captures_iter(&code) {
        let call = &c[1];
        if !ROUTER_RS_CALLS.iter().any(|(allowed, _)| *allowed == call) {
            problems.push(format!("calls `{call}(..)`"));
        }
    }
    assert!(
        problems.is_empty(),
        "\n\nrouter.rs lists route modules and adds cross-cutting layers only. Mount a \
         module's routes through its `routes(ctx)`:\n  - {}\n",
        problems.join("\n  - ")
    );
}

#[test]
fn routes_are_registered_only_in_routes_rs() {
    let mut files = Vec::new();
    files.extend(crate::support::sources::rs_files());
    let call_re =
        regex::Regex::new(r"\.(route|routes|nest|nest_service|fallback|fallback_service)\(")
            .unwrap();
    let mut problems = Vec::new();
    for f in &files {
        let r = rel(f);
        if r == "router.rs"
            || r.ends_with("/routes.rs")
            || REGISTRATION_ALLOWLIST.iter().any(|(file, _)| *file == r)
        {
            continue;
        }
        let code = production_code(&fs::read_to_string(f).unwrap());
        for (n, line) in code.lines().enumerate() {
            if let Some(m) = call_re.find(line) {
                problems.push(format!("{r}: `{}` ({})", m.as_str(), line.trim()));
                let _ = n;
            }
        }
    }
    assert!(
        problems.is_empty(),
        "\n\nRoutes are registered in the owning module's routes.rs (its `routes(ctx)` \
         and the handler lists it mounts), nowhere else:\n  - {}\n",
        problems.join("\n  - ")
    );
}

#[test]
fn every_routes_rs_has_the_entry_point_and_router_rs_mounts_each_once() {
    let mut files = Vec::new();
    files.extend(crate::support::sources::rs_files());
    let entry_re =
        regex::Regex::new(r"(?m)^pub fn routes\(ctx: &PlatformContext\) -> AggregateRoutes \{")
            .unwrap();
    let mut modules = BTreeSet::new();
    let mut problems = Vec::new();
    for f in &files {
        let r = rel(f);
        if !r.ends_with("/routes.rs") {
            continue;
        }
        let module = r.trim_end_matches("/routes.rs").replace('/', "::");
        if entry_re.is_match(&fs::read_to_string(f).unwrap()) {
            modules.insert(module);
        } else {
            problems.push(format!(
                "{r} has no `pub fn routes(ctx: &PlatformContext) -> AggregateRoutes`"
            ));
        }
    }

    let router = fs::read_to_string(crate::support::sources::file("router.rs")).unwrap();
    let mount_re = regex::Regex::new(r"crate::([a-z_:]+?)::routes\(ctx\)").unwrap();
    let mut mounted = Vec::new();
    for c in mount_re.captures_iter(&production_code(&router)) {
        mounted.push(c[1].to_string());
    }
    for m in &modules {
        match mounted.iter().filter(|x| *x == m).count() {
            1 => {}
            0 => problems.push(format!("router.rs does not mount crate::{m}::routes(ctx)")),
            n => problems.push(format!(
                "router.rs mounts crate::{m}::routes(ctx) {n} times"
            )),
        }
    }
    for m in &mounted {
        if !modules.contains(m) {
            problems.push(format!(
                "router.rs mounts crate::{m}::routes(ctx), which has no routes.rs"
            ));
        }
    }
    assert!(
        modules.len() > 20,
        "only {} route modules found",
        modules.len()
    );
    assert!(
        problems.is_empty(),
        "\n\nEach route module's routes.rs has one entry point and router.rs mounts it \
         once:\n  - {}\n",
        problems.join("\n  - ")
    );
}

#[test]
fn router_rs_mounts_only_modules_and_cross_cutting_routers() {
    let router = fs::read_to_string(crate::support::sources::file("router.rs")).unwrap();
    let code = production_code(&router);
    let build = fn_body(&code, "build");
    let merge_re = regex::Regex::new(r"\.merge\(").unwrap();
    let module_entry = regex::Regex::new(r"^crate::[a-z_:]+::routes\(ctx\)$").unwrap();
    let mut problems = Vec::new();
    for m in merge_re.find_iter(build) {
        let open = m.end() - 1;
        let end = matching(build.as_bytes(), open);
        let arg = build[open + 1..end - 1]
            .split_whitespace()
            .collect::<String>();
        let ok = module_entry.is_match(&arg)
            || ROUTER_RS_CALLS
                .iter()
                .any(|(call, _)| arg.starts_with(&format!("{call}(")))
            || arg == "router"
            || arg == "plain"
            || arg.starts_with("crate::shared::openapi_contract::")
            || arg.starts_with("SwaggerUi::");
        if !ok {
            problems.push(arg);
        }
    }
    assert!(
        problems.is_empty(),
        "\n\nrouter.rs::build merges something that is neither a module's routes(ctx) nor \
         a cross-cutting router:\n  - {}\n",
        problems.join("\n  - ")
    );
}
