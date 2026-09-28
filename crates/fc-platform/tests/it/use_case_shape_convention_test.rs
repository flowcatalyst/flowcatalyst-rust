//! Convention tests for the use-case shape (platform-uniformity phase 2,
//! `docs/architecture/use-case-template.md`).
//!
//! - **authorize**: every `impl UseCase` answers "may *this caller* act on
//!   *this target*?" in `authorize`, from `ctx.caller()`. An `authorize`
//!   that is just `Ok(())` needs an entry in [`EMPTY_AUTHORIZE`] with the
//!   reason it has nothing to check, and every entry must still be empty
//!   (so the list never goes stale).
//! - **shape**: a use-case file reads command, then the use-case struct and
//!   its `new`, then `impl UseCase` with `validate`, `authorize` and
//!   `execute` in that order. Real exceptions are listed in
//!   [`SHAPE_EXCEPTIONS`] with a reason.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

/// Use cases whose `authorize` is `Ok(())`, keyed `path::UseCase` (path
/// relative to `src/`), with why there is nothing for it to check.
const EMPTY_AUTHORIZE: &[(&str, &str)] = &[
    (
        "app_docs/operations/sync.rs::SyncAppDocsUseCase",
        "application scope only: the SDK route resolves `/{appCode}` within the caller's \
         scope before the body (404, owner ruling: no existence oracle)",
    ),
    (
        "application_openapi_spec/operations/sync.rs::SyncOpenApiSpecUseCase",
        "application scope only: the SDK route resolves `/{appCode}` within the caller's \
         scope before the body (404); the platform sync is anchor-gated or the system caller",
    ),
    (
        "event_type/operations/sync.rs::SyncEventTypesUseCase",
        "application scope only: the SDK route resolves `/{appCode}` within the caller's \
         scope before the body (404); the platform sync is anchor-gated or the system caller \
         (Go's is Public too)",
    ),
    (
        "process/operations/sync.rs::SyncProcessesUseCase",
        "application scope only: both SDK routes resolve the application within the caller's \
         scope before the body (404, owner ruling; Go answers 403 from its authorize)",
    ),
    (
        "function/operations/delete.rs::DeleteFunctionUseCase",
        "reach is load-or-404 at the top of execute (Java `Access.byAddress`: out of reach is \
         the same 404, so the check is the load)",
    ),
    (
        "function/operations/update.rs::UpdateFunctionUseCase",
        "reach is load-or-404 at the top of execute (Java `Access.byAddress`)",
    ),
    (
        "function/operations/promote.rs::PromoteVersionUseCase",
        "reach is load-or-404 at the top of execute (Java `Access.byAddress`)",
    ),
    (
        "function/operations/promote.rs::RemoveAliasUseCase",
        "reach is load-or-404 at the top of execute (Java `Access.byAddress`)",
    ),
    (
        "function/operations/publish.rs::PublishVersionUseCase",
        "reach is load-or-404 at the top of execute (Java `Access.byAddress`)",
    ),
    (
        "function/operations/retire.rs::RetireVersionUseCase",
        "reach is load-or-404 at the top of execute (Java `Access.byAddress`)",
    ),
    (
        "function/operations/settings.rs::SetFunctionConfigUseCase",
        "reach is load-or-404 at the top of execute (Java `Access.byAddress`)",
    ),
    (
        "function/operations/settings.rs::SetFunctionSecretUseCase",
        "reach is load-or-404 at the top of execute (Java `Access.byAddress`)",
    ),
    (
        "function/operations/settings.rs::DeleteFunctionSecretUseCase",
        "reach is load-or-404 at the top of execute (Java `Access.byAddress`)",
    ),
    (
        "function/operations/domains.rs::ReleaseFunctionDomainUseCase",
        "reach is load-or-404 at the top of execute (Java `Access.byHostname`)",
    ),
    (
        "function/operations/mark_ready.rs::MarkVersionReadyUseCase",
        "platform-internal: a function host reports a version loaded over the control plane, \
         gated by its host credential; no principal target to reach",
    ),
    (
        "webauthn/operations/authenticate_with_passkey.rs::AuthenticatePasskeyUseCase",
        "pre-login: runs as the system caller before anyone is authenticated; the WebAuthn \
         assertion is the credential, checked in execute",
    ),
];

/// Use-case files or use cases that don't follow the template's layout,
/// keyed as in [`EMPTY_AUTHORIZE`] (a use case) or by path (the whole
/// file), with the reason.
const SHAPE_EXCEPTIONS: &[(&str, &str)] = &[
    (
        "function/operations/create.rs",
        "built by `FunctionOperations` (function/operations/mod.rs), which owns the \
         dependencies the function use cases share (Java's single `FunctionOperations`) \
         and hands each its unit of work; no per-use-case `new`",
    ),
    (
        "function/operations/delete.rs",
        "built by `FunctionOperations`, as create.rs",
    ),
    (
        "function/operations/domains.rs",
        "built by `FunctionOperations`, as create.rs",
    ),
    (
        "function/operations/promote.rs",
        "built by `FunctionOperations`, as create.rs",
    ),
    (
        "function/operations/publish.rs",
        "built by `FunctionOperations`, as create.rs",
    ),
    (
        "function/operations/put_policy.rs",
        "built by `FunctionOperations`, as create.rs",
    ),
    (
        "function/operations/retire.rs",
        "built by `FunctionOperations`, as create.rs",
    ),
    (
        "function/operations/settings.rs",
        "built by `FunctionOperations`, as create.rs",
    ),
    (
        "function/operations/update.rs",
        "built by `FunctionOperations`, as create.rs",
    ),
];

/// `s` without `//` comments (line and doc) and whitespace.
fn squeezed(s: &str) -> String {
    s.lines()
        .map(|l| match l.find("//") {
            Some(i) => &l[..i],
            None => l,
        })
        .collect::<String>()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

/// The text from `open` (a `{`) to its matching `}`, and the index after it.
fn balanced(s: &str, open: usize) -> (&str, usize) {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    for (i, &b) in bytes.iter().enumerate().skip(open) {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return (&s[open + 1..i], i + 1);
                }
            }
            _ => {}
        }
    }
    (&s[open + 1..], s.len())
}

/// One `impl UseCase for X` in a file.
struct UseCaseImpl {
    name: String,
    /// Byte offset of the `impl` line.
    at: usize,
    command: String,
    /// The `fn` names of the impl, in order.
    fns: Vec<String>,
    authorize_body: Option<String>,
}

fn is_use_case_impl(line: &str) -> bool {
    let line = line.trim_start();
    (line.starts_with("impl ") || line.starts_with("impl<"))
        && line
            .match_indices("UseCase for ")
            .any(|(i, _)| i > 0 && matches!(line.as_bytes()[i - 1], b' ' | b'>'))
}

fn use_case_impls(content: &str) -> Vec<UseCaseImpl> {
    let mut out = Vec::new();
    let mut offset = 0;
    for line in content.split_inclusive('\n') {
        if is_use_case_impl(line) {
            let name = line
                .split("UseCase for ")
                .nth(1)
                .unwrap_or("")
                .split([' ', '<', '{'])
                .find(|s| !s.is_empty())
                .unwrap_or("")
                .to_string();
            let open = offset + line.find('{').unwrap_or(line.len() - 1);
            let (body, _) = balanced(content, open);
            let command = body
                .split("type Command")
                .nth(1)
                .and_then(|r| r.split(';').next())
                .map(|c| c.trim_start_matches([' ', '=']).trim().to_string())
                .unwrap_or_default();
            let fns: Vec<String> = body
                .match_indices("fn ")
                .filter(|(i, _)| *i == 0 || matches!(body.as_bytes()[i - 1], b' ' | b'\n' | b'\t'))
                .map(|(i, _)| {
                    body[i + 3..]
                        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                        .next()
                        .unwrap_or("")
                        .to_string()
                })
                .collect();
            let authorize_body = body.find("fn authorize").map(|i| {
                let arrow = body[i..].find("->").map_or(i, |a| i + a);
                let open = body[arrow..].find('{').map_or(arrow, |o| arrow + o);
                balanced(body, open).0.to_string()
            });
            out.push(UseCaseImpl {
                name,
                at: offset,
                command,
                fns,
                authorize_body,
            });
        }
        offset += line.len();
    }
    out
}

/// Every use case in `src/`, keyed `path::UseCase`, with its file text.
fn all_use_cases() -> BTreeMap<String, (String, UseCaseImpl)> {
    let mut files = Vec::new();
    files.extend(crate::support::sources::rs_files());
    let mut out = BTreeMap::new();
    for file in files {
        let Ok(content) = fs::read_to_string(&file) else {
            continue;
        };
        let rel = crate::support::sources::strip_src(&file)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        for uc in use_case_impls(&content) {
            out.insert(format!("{rel}::{}", uc.name), (content.clone(), uc));
        }
    }
    out
}

#[test]
fn every_use_case_authorizes_or_says_why_not() {
    let use_cases = all_use_cases();
    assert!(
        use_cases.len() >= 140,
        "the scanner found only {} use cases",
        use_cases.len()
    );
    let allowed: BTreeMap<&str, &str> = EMPTY_AUTHORIZE.iter().copied().collect();
    let mut unlisted = Vec::new();
    let mut stale = Vec::new();
    let mut missing = Vec::new();
    let mut real = 0;
    for (key, (_, uc)) in &use_cases {
        let Some(body) = &uc.authorize_body else {
            missing.push(key.clone());
            continue;
        };
        let empty = squeezed(body) == "Ok(())";
        match (empty, allowed.contains_key(key.as_str())) {
            (true, false) => unlisted.push(key.clone()),
            (false, true) => stale.push(key.clone()),
            (false, false) => real += 1,
            (true, true) => {}
        }
    }
    for key in allowed.keys() {
        if !use_cases.contains_key(*key) {
            stale.push(format!("{key} (no such use case)"));
        }
    }
    println!(
        "{} use cases: {real} authorize, {} allowlisted",
        use_cases.len(),
        allowed.len()
    );
    let mut msg = String::new();
    if !unlisted.is_empty() {
        msg.push_str(
            "\nUse cases whose `authorize` is an empty `Ok(())`. Check the resource rule here \
             (client reach, application scope, anchor-only, ownership, ceilings) from \
             `ctx.caller()`, or add the use case to EMPTY_AUTHORIZE with the reason it has \
             nothing to check:\n",
        );
        for k in &unlisted {
            msg.push_str(&format!("  - {k}\n"));
        }
    }
    if !stale.is_empty() {
        msg.push_str(
            "\nStale EMPTY_AUTHORIZE entries (the use case now authorizes, or is gone):\n",
        );
        for k in &stale {
            msg.push_str(&format!("  - {k}\n"));
        }
    }
    if !missing.is_empty() {
        msg.push_str("\nUse cases with no `authorize` the scanner could find:\n");
        for k in &missing {
            msg.push_str(&format!("  - {k}\n"));
        }
    }
    assert!(msg.is_empty(), "{msg}");
}

#[test]
fn use_cases_follow_the_template_shape() {
    let use_cases = all_use_cases();
    let exceptions: BTreeSet<&str> = SHAPE_EXCEPTIONS.iter().map(|(k, _)| *k).collect();
    let mut violations = Vec::new();
    for (key, (content, uc)) in &use_cases {
        let file = key.split("::").next().unwrap_or("");
        if exceptions.contains(key.as_str()) || exceptions.contains(file) {
            continue;
        }
        // validate → authorize → execute, in that order, and nothing else
        // (`run` is the trait's; overriding it would skip a step).
        let steps: Vec<&str> = uc
            .fns
            .iter()
            .map(String::as_str)
            .filter(|f| matches!(*f, "validate" | "authorize" | "execute" | "run"))
            .collect();
        if steps != ["validate", "authorize", "execute"] {
            violations.push(format!(
                "{key}: impl UseCase has {steps:?}, not validate, authorize, execute"
            ));
        }
        // The use-case struct, then its `new`, both before the impl.
        let struct_at = content
            .find(&format!("pub struct {}", uc.name))
            .or_else(|| content.find(&format!("struct {}", uc.name)));
        let new_at = content
            .match_indices(&format!("impl<U: UnitOfWork> {}<U>", uc.name))
            .chain(content.match_indices(&format!("impl {} {{", uc.name)))
            .map(|(i, _)| i)
            .find(|&i| {
                content[i..]
                    .split("\n}")
                    .next()
                    .is_some_and(|b| b.contains("fn new("))
            });
        match (struct_at, new_at) {
            (Some(s), Some(n)) if s < n && n < uc.at => {}
            (None, _) => violations.push(format!("{key}: no `struct {}` in its file", uc.name)),
            (_, None) => violations.push(format!(
                "{key}: no `impl {} {{ pub fn new(..) }}` before the `impl UseCase`",
                uc.name
            )),
            _ => violations.push(format!(
                "{key}: the struct and its `new` must come before the `impl UseCase`"
            )),
        }
        // A command defined in the same file comes before the struct.
        let command = uc.command.split('<').next().unwrap_or("");
        if let (Some(cmd_at), Some(s)) = (content.find(&format!("pub struct {command}")), struct_at)
        {
            if cmd_at > s {
                violations.push(format!(
                    "{key}: the command `{command}` must be declared before the use-case struct"
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "\nUse cases that don't follow docs/architecture/use-case-template.md (command, \
         struct + new, impl UseCase with validate/authorize/execute in order). Fix them, or \
         list a real exception in SHAPE_EXCEPTIONS with its reason:\n  - {}\n",
        violations.join("\n  - ")
    );
}
