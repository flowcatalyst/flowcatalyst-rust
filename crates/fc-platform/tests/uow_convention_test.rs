//! Convention test: every `*UseCase::execute` body must terminate through
//! `UnitOfWork::commit` / `commit_delete` / `emit_event`, OR only return
//! `UseCaseResult::failure`s.
//!
//! `UseCaseResult::success` is sealed in the `usecase` module, so it's
//! impossible to construct a success outside of UoW — but a use case could
//! still do a direct repo write and then emit no event, and technically
//! compile (it would only return `failure` at the end). This test catches
//! that anti-pattern.
//!
//! Complements `permission_convention_test.rs`, which checks handler-level
//! authorization. Together the two cover the structural write pipeline:
//! handler gates the caller, use case gates the write.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Any of these substrings in the execute body means the use case is
/// routing through UnitOfWork — the happy path we want.
const UOW_PATTERNS: &[&str] = &[
    "unit_of_work.commit(",
    "unit_of_work.commit_delete(",
    "unit_of_work.commit_all(",
    "unit_of_work.emit_event(",
];

/// File-level skip list: use-case files that don't own writes (e.g. pure
/// read/query use cases, if any are ever added).
const FILE_SKIPLIST: &[&str] = &[];

/// `"path/suffix::fn_name"` — specific execute methods to skip.
const FN_SKIPLIST: &[&str] = &[];

fn src_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn should_skip(path: &Path) -> bool {
    let rel = path
        .strip_prefix(src_root())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    FILE_SKIPLIST.iter().any(|s| rel.contains(s))
}

fn walk_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk_rs_files(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(p);
        }
    }
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
        if line.contains("impl ") && line.contains(" UseCase for ") {
            // Extract the struct name.
            let after = line.split(" UseCase for ").nth(1).unwrap_or("");
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

fn has_uow_call(body: &str) -> bool {
    UOW_PATTERNS.iter().any(|p| body.contains(p))
}

/// The body only returns failures (and never reaches a success path) —
/// acceptable, because the seal prevents fabricating success. Detected
/// heuristically: every terminal expression is a `UseCaseResult::failure`
/// or propagates one.
fn only_returns_failures(body: &str) -> bool {
    // If the body never contains a success-producing pattern AND never
    // returns anything other than failure, it's trivially-compliant: the
    // compiler would reject it otherwise. Simpler heuristic: the body
    // mentions `UseCaseResult::failure` but NOT any success-producing
    // expression (which, thanks to the seal, can only be a UoW call).
    //
    // Returns true only if every path is a failure. Conservative: if the
    // body mentions any success-producing expression — the restricted
    // `UseCaseResult::success(` constructor, a direct `UseCaseResult(Ok(..))`
    // construction, a legacy `UseCaseResult::Success(` variant, or
    // `result.map(` etc — treat it as trying to produce a success and
    // require UoW.
    const SUCCESS_PATTERNS: &[&str] = &[
        "UseCaseResult::success(",
        "UseCaseResult(Ok",
        "UseCaseResult::Success",
        ".map(|",
    ];
    body.contains("UseCaseResult::failure")
        && !SUCCESS_PATTERNS.iter().any(|p| body.contains(p))
        && !has_uow_call(body)
}

#[test]
fn every_use_case_terminates_through_unit_of_work() {
    let skip_keys: HashSet<&str> = FN_SKIPLIST.iter().copied().collect();

    let mut files = Vec::new();
    walk_rs_files(&src_root(), &mut files);

    let mut violations = Vec::new();

    for file in &files {
        if should_skip(file) {
            continue;
        }
        let Ok(content) = fs::read_to_string(file) else {
            continue;
        };
        let rel = file
            .strip_prefix(src_root())
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");

        for (qualified, body) in extract_use_case_execute_bodies(&content) {
            let key = format!("{}::{}", rel, qualified);
            if skip_keys.contains(key.as_str()) {
                continue;
            }
            if has_uow_call(&body) {
                continue;
            }
            if only_returns_failures(&body) {
                continue;
            }
            violations.push(format!("{}  ({})", qualified, rel));
        }
    }

    if !violations.is_empty() {
        let mut msg = String::from(
            "\n\nUse cases whose `execute` body doesn't terminate through `UnitOfWork`.\n\
             Every `*UseCase::execute` must either call one of \
             `unit_of_work.commit/commit_delete/emit_event` on the happy path, \
             or return only `UseCaseResult::failure`.\n\
             Success can only be constructed inside the `usecase` module; skipping UoW \
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
