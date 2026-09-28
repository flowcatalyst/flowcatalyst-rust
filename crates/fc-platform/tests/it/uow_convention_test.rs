//! Convention test: every `*UseCase::execute` body must terminate through
//! `UnitOfWork::commit` / `commit_delete` / `commit_all` / `emit_event` /
//! `emit_events` / `commit_all_with_events` / `commit_sync`, OR only return
//! `Err(..)`s. A sync use case writes nothing outside that commit.
//!
//! `execute` returns `Result<Committed<Event>, UseCaseError>`, and
//! `Committed` can only be constructed inside the `usecase` module, so it's
//! impossible to produce a success outside of UoW — but a use case could
//! still do a direct repo write and then emit no event, and technically
//! compile (it would only return `Err` at the end). This test catches
//! that anti-pattern.
//!
//! Complements `permission_convention_test.rs`, which checks handler-level
//! authorization. Together the two cover the structural write pipeline:
//! handler gates the caller, use case gates the write.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// Any of these substrings in the execute body means the use case is
/// routing through UnitOfWork — the happy path we want.
const UOW_PATTERNS: &[&str] = &[
    "unit_of_work.commit(",
    "unit_of_work.commit_delete(",
    "unit_of_work.commit_all(",
    "unit_of_work.emit_event(",
    "unit_of_work.emit_events(",
    "unit_of_work.commit_all_with_events(",
    "unit_of_work.commit_sync(",
];

/// File-level skip list: use-case files that don't own writes (e.g. pure
/// read/query use cases, if any are ever added).
const FILE_SKIPLIST: &[&str] = &[];

/// `"path/suffix::fn_name"` — specific execute methods to skip.
const FN_SKIPLIST: &[&str] = &[];

fn should_skip(path: &Path) -> bool {
    let rel = crate::support::sources::strip_src(path)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    FILE_SKIPLIST.iter().any(|s| rel.contains(s))
}

/// `impl UseCase for X` or `impl<U: UnitOfWork> UseCase for X<U>` — the
/// trait name preceded by a space or the generics' `>`, never part of a
/// longer name.
fn is_use_case_impl(line: &str) -> bool {
    let line = line.trim_start();
    (line.starts_with("impl ") || line.starts_with("impl<"))
        && line
            .match_indices("UseCase for ")
            .any(|(i, _)| i > 0 && matches!(line.as_bytes()[i - 1], b' ' | b'>'))
}

/// Extract the bodies of every `execute` method that's part of an
/// `impl<...> UseCase for X<...>` block.
///
/// Returns a list of (fn_name_qualified, body_text) pairs. The qualifier is
/// the enclosing struct name so errors are readable (e.g. `CreateUserUseCase::execute`).
fn extract_use_case_execute_bodies(content: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = content.lines().collect();
    let mut out = Vec::new();

    // Scan line by line for `impl <...> UseCase for <Struct>`
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if is_use_case_impl(line) {
            // Extract the struct name.
            let after = line.split("UseCase for ").nth(1).unwrap_or("");
            let struct_name = after
                .split([' ', '<', '{'])
                .find(|s| !s.is_empty())
                .unwrap_or("")
                .to_string();

            // Walk forward to find `async fn execute(` inside this impl.
            let mut depth = 0i32;
            let mut started = false;
            let mut j = i;
            while j < lines.len() {
                let l = lines[j];
                for ch in l.chars() {
                    if ch == '{' {
                        started = true;
                        depth += 1;
                    } else if ch == '}' {
                        depth -= 1;
                    }
                }
                if l.contains("async fn execute(") || l.contains("fn execute(") {
                    // Read the execute body.
                    let (body, body_end) = read_balanced_body(&lines, j);
                    out.push((format!("{}::execute", struct_name), body));
                    j = body_end;
                    continue;
                }
                j += 1;
                if started && depth == 0 {
                    break;
                }
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

fn read_balanced_body(lines: &[&str], start_line: usize) -> (String, usize) {
    let mut depth = 0i32;
    let mut started = false;
    let mut body = String::new();
    let mut i = start_line;
    while i < lines.len() {
        let l = lines[i];
        body.push_str(l);
        body.push('\n');
        for ch in l.chars() {
            if ch == '{' {
                started = true;
                depth += 1;
            } else if ch == '}' {
                depth -= 1;
            }
        }
        i += 1;
        if started && depth == 0 {
            break;
        }
    }
    (body, i)
}

/// `body` with its whitespace removed, so a call rustfmt splits across
/// lines (`self.unit_of_work\n    .commit(`) still matches.
fn squeezed(body: &str) -> String {
    body.chars().filter(|c| !c.is_whitespace()).collect()
}

fn has_uow_call(body: &str) -> bool {
    let body = squeezed(body);
    UOW_PATTERNS.iter().any(|p| body.contains(p))
}

/// The body only returns failures (and never reaches a success path) —
/// acceptable, because the seal prevents fabricating success. Detected
/// heuristically: the body returns an `Err(..)` and mentions nothing that
/// could carry a success.
///
/// Conservative: if the body mentions any success-producing expression —
/// the restricted `Committed::new(` constructor, a direct `Committed(..)`
/// construction, a sealed outcome re-used through `.into_committed()`, an
/// `Ok(..)`, or a `.map(|..|` over one — treat it as trying to produce a
/// success and require UoW.
fn only_returns_failures(body: &str) -> bool {
    const SUCCESS_PATTERNS: &[&str] = &[
        "Committed::new(",
        "Committed(",
        ".into_committed(",
        "Ok(",
        ".map(|",
    ];
    let squeezed = squeezed(body);
    squeezed.contains("Err(")
        && !SUCCESS_PATTERNS.iter().any(|p| squeezed.contains(p))
        && !has_uow_call(body)
}

#[test]
fn every_use_case_terminates_through_unit_of_work() {
    let skip_keys: HashSet<&str> = FN_SKIPLIST.iter().copied().collect();

    let mut files = Vec::new();
    files.extend(crate::support::sources::rs_files());

    let mut violations = Vec::new();
    let mut checked = 0usize;
    let mut through_uow = 0usize;

    for file in &files {
        if should_skip(file) {
            continue;
        }
        let Ok(content) = fs::read_to_string(file) else {
            continue;
        };
        let rel = crate::support::sources::strip_src(file)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");

        for (qualified, body) in extract_use_case_execute_bodies(&content) {
            let key = format!("{}::{}", rel, qualified);
            if skip_keys.contains(key.as_str()) {
                continue;
            }
            checked += 1;
            if has_uow_call(&body) {
                through_uow += 1;
                continue;
            }
            if only_returns_failures(&body) {
                continue;
            }
            violations.push(format!("{}  ({})", qualified, rel));
        }
    }

    println!("{checked} execute bodies, {through_uow} through UnitOfWork");
    // Guard the scanner itself: if it stops finding `execute` bodies (or
    // stops recognising the UoW calls in them), the test would pass
    // vacuously.
    assert!(
        checked >= 100 && through_uow * 10 >= checked * 9,
        "the scanner found {checked} execute bodies, {through_uow} through UnitOfWork"
    );

    if !violations.is_empty() {
        let mut msg = String::from(
            "\n\nUse cases whose `execute` body doesn't terminate through `UnitOfWork`.\n\
             Every `*UseCase::execute` must either call one of \
             `unit_of_work.commit/commit_delete/commit_all/emit_event/emit_events/\
             commit_all_with_events/commit_sync` on the happy path, or return only `Err(..)`.\n\
             `Committed` can only be constructed inside the `usecase` module; skipping UoW \
             means the use case never emits a domain event or audit log — a silent data \
             integrity bug.\n\n\
             Violators:\n",
        );
        for v in &violations {
            msg.push_str("  - ");
            msg.push_str(v);
            msg.push('\n');
        }
        panic!("{}", msg);
    }
}

/// A sync is planned in full and written in one unit-of-work transaction
/// with its per-row events and rollup (Go's `usecaseop.Sync`), so a bad row
/// fails the sync with nothing written. A repository write called from a
/// sync use case would land outside that transaction and survive a later
/// row's failure.
#[test]
fn sync_use_cases_write_only_through_the_unit_of_work() {
    let mut files = Vec::new();
    files.extend(crate::support::sources::rs_files());
    let writes = regex::Regex::new(
        r"(?:_repo|\brepo)\s*\.\s*(?:insert|update|delete|upsert|save|archive\w*|insert_\w+|update_\w+|delete_\w+)\s*\(",
    )
    .unwrap();
    let mut syncs = 0;
    let mut violations = Vec::new();
    for path in files {
        let is_sync = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("sync"))
            && path
                .parent()
                .and_then(|p| p.file_name())
                .is_some_and(|n| n == "operations");
        if !is_sync {
            continue;
        }
        syncs += 1;
        let content = fs::read_to_string(&path).unwrap_or_default();
        let code = content.split("#[cfg(test)]").next().unwrap_or("");
        for (n, line) in code.lines().enumerate() {
            if !line.trim_start().starts_with("//") && writes.is_match(line) {
                violations.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
            }
        }
    }
    assert!(syncs >= 10, "found only {syncs} sync use cases");
    assert!(
        violations.is_empty(),
        "sync use cases writing outside the unit of work:\n{}",
        violations.join("\n")
    );
}
