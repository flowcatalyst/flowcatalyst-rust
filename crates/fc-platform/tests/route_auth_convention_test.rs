//! Convention test: every route the platform mounts under `/api` and `/bff`
//! authenticates its caller and checks its authority, unless it is on an
//! explicit allowlist below with the reason.
//!
//! The route table is read from the source the way the router is built:
//! `router.rs` nests each resource router at its prefix; a resource router
//! declares `.route("/path", get(h).post(h2))` (plain axum) or
//! `.routes(routes!(h, h2))` (utoipa, whose method and path are in each
//! handler's `#[utoipa::path(...)]`), and may nest or merge further routers.
//! The scan follows all of it, so a route added anywhere is checked without
//! anyone remembering to list it.
//!
//! Three rules:
//! 1. **Authentication.** The handler extracts `Authenticated` (bearer or
//!    session cookie). `OptionalAuth` or no extractor at all needs an entry in
//!    [`PUBLIC_ROUTES`].
//! 2. **Writes check authority.** A POST/PUT/PATCH/DELETE handler calls a
//!    permission check (CLAUDE.md: the URL tier is no second line of
//!    defence), or is in [`WRITES_WITHOUT_PERMISSION`].
//! 3. **Reads check authority.** A GET handler calls a `can_read_*` (or
//!    another permission check), as Go does, or is in
//!    [`READS_WITHOUT_PERMISSION`] because Go also lets any signed-in caller
//!    read it (the caller's own profile, client list, …).
//!
//! Run with `ROUTE_INVENTORY=1 ... -- --nocapture` to print the route table.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// Routes that authenticate no one, or only optionally: `"METHOD /path"`.
const PUBLIC_ROUTES: &[(&str, &str)] = &[
    (
        "GET /api/public/platform",
        "the login page's platform info (Go publicapi)",
    ),
    (
        "GET /api/public/login-theme",
        "the login page's theme (Go publicapi)",
    ),
    (
        "GET /api/config/platform",
        "the SPA's feature flags before login (Go publicapi)",
    ),
    (
        "POST /api/dispatch/process",
        "the message router's callback, authenticated by the handler's own router credential check",
    ),
    (
        "POST /api/dispatch/settled",
        "the message router's report of settled siblings, authenticated per job by the scheduler's HMAC token (Go settled.Handler)",
    ),
    (
        "GET /api/openapi-functions.json",
        "the function API contract, unauthenticated as in Java (FunctionOpenApiRoutes)",
    ),
    (
        "GET /api/schemas/function-manifest.json",
        "the manifest JSON Schema an editor fetches with no token, as in Java",
    ),
];

/// Writes whose handler needs no permission check: `"METHOD /path"`.
const WRITES_WITHOUT_PERMISSION: &[(&str, &str)] = &[];

/// Reads Go lets any signed-in caller make (`docs/parity/read-permissions-vs-go.md`):
/// `"METHOD /path"`. The profile-only gate still refuses them to a USER with
/// no role.
const READS_WITHOUT_PERMISSION: &[(&str, &str)] = &[
    ("GET /api/me", "the caller's own identity (Go me.whoami)"),
    (
        "GET /api/me/applications",
        "the caller's own applications (Go me.listMyApplications)",
    ),
    (
        "GET /api/me/clients",
        "the caller's own clients (Go me.listMyClients)",
    ),
    (
        "GET /api/me/clients/{clientId}",
        "one of the caller's own clients (Go me.getMyClient)",
    ),
    (
        "GET /api/me/clients/{clientId}/applications",
        "a reachable client's applications (Go me.listMyClientApplications)",
    ),
    (
        "GET /api/clients/{id}/applications",
        "Go getApplications: anchor or client reach, no permission",
    ),
    (
        "GET /api/platform/cors/allowed",
        "Go publicAllowed checks nothing",
    ),
    (
        "GET /api/email-domain-mappings/lookup/{domain}",
        "Go's mapping lookup checks nothing",
    ),
    ("GET /bff/roles", "Go bff/roles.go list checks nothing"),
    (
        "GET /bff/roles/{roleName}",
        "Go bff/roles.go get checks nothing",
    ),
    (
        "GET /bff/roles/filters/applications",
        "Go bff/roles.go filterApplications checks nothing",
    ),
    (
        "GET /bff/roles/permissions",
        "Go bff/roles.go listPermissions checks nothing",
    ),
    (
        "GET /bff/roles/permissions/{permission}",
        "Go bff/roles.go getPermission checks nothing",
    ),
    (
        "GET /bff/event-types/filters/applications",
        "Go bff filter_options.go eventTypeApplications checks nothing",
    ),
    (
        "GET /bff/event-types/filters/subdomains",
        "Go bff/event_types.go filterSubdomains checks nothing",
    ),
    (
        "GET /bff/event-types/filters/aggregates",
        "Go bff/event_types.go filterAggregates checks nothing",
    ),
    (
        "GET /bff/filter-options",
        "filter dropdowns confined to the caller's clients, as Go's /bff/filter-options/clients",
    ),
    (
        "GET /bff/filter-options/clients",
        "Go clientOptions: the caller's reachable clients, no permission",
    ),
    (
        "GET /bff/filter-options/dispatch-jobs",
        "filter dropdowns confined to the caller's clients, as Go's /bff/filter-options/clients",
    ),
    (
        "GET /bff/filter-options/dispatch-pools",
        "filter dropdowns confined to the caller's clients, as Go's /bff/filter-options/clients",
    ),
    (
        "GET /bff/filter-options/events",
        "filter dropdowns confined to the caller's clients, as Go's /bff/filter-options/clients",
    ),
    (
        "GET /bff/filter-options/subscriptions",
        "filter dropdowns confined to the caller's clients, as Go's /bff/filter-options/clients",
    ),
    (
        "GET /bff/filter-options/event-types",
        "event-type code facets, as Go's /bff/event-types/filters/*",
    ),
    (
        "GET /bff/filter-options/event-types/filters/applications",
        "event-type code facets, as Go's /bff/event-types/filters/*",
    ),
    (
        "GET /bff/filter-options/event-types/filters/subdomains",
        "event-type code facets, as Go's /bff/event-types/filters/*",
    ),
    (
        "GET /bff/filter-options/event-types/filters/aggregates",
        "event-type code facets, as Go's /bff/event-types/filters/*",
    ),
];

/// What counts as a read gate: a permission or anchor check, never a bare
/// client-reach filter (that confines rows; it does not authorize the read).
const READ_CHECK_PATTERNS: &[&str] = &[
    "can_read_",
    "can_view_",
    "require_permission",
    "require_anchor",
    "is_admin(",
    ".authorize(",
    ".has_permission(",
    ".has_any_permission(",
];

/// Any of these in a handler body (or a same-file helper it calls) counts as
/// an authority check. Kept in step with `permission_convention_test.rs`.
const AUTH_CHECK_PATTERNS: &[&str] = &[
    "require_anchor",
    "require_permission",
    "require_client_access",
    "is_admin(",
    "can_read_",
    "can_write_",
    "can_create_",
    "can_update_",
    "can_delete_",
    "can_retry_",
    "can_pause_",
    "can_resume_",
    "can_fire_",
    "can_activate_",
    "can_suspend_",
    "can_deactivate_",
    "can_grant_",
    "can_revoke_",
    "can_assign_",
    "can_administer_",
    "can_sync_",
    "can_view_",
    ".authorize(",
    ".is_anchor()",
    ".can_access_client(",
    ".has_permission(",
    ".has_any_permission(",
];

// ─── Source model ──────────────────────────────────────────────────────────

struct SourceFn {
    file: String,
    name: String,
    /// Text from `fn` to the opening `{` of the body.
    signature: String,
    body: String,
    /// The `#[utoipa::path(...)]` attribute right above it, if any.
    utoipa: Option<String>,
}

struct Source {
    files: HashMap<String, String>,
    fns: Vec<SourceFn>,
    by_name: HashMap<String, Vec<usize>>,
}

fn src_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
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

/// Index of the byte after the token that starts at `i` when it is a
/// comment, string or char literal; `None` when `i` starts none of those.
fn skip_literal(s: &[u8], i: usize) -> Option<usize> {
    let at = |k: usize| s.get(k).copied().unwrap_or(0);
    match at(i) {
        b'/' if at(i + 1) == b'/' => {
            let mut j = i;
            while j < s.len() && s[j] != b'\n' {
                j += 1;
            }
            Some(j)
        }
        b'/' if at(i + 1) == b'*' => {
            let mut j = i + 2;
            while j + 1 < s.len() && !(s[j] == b'*' && s[j + 1] == b'/') {
                j += 1;
            }
            Some(j + 2)
        }
        b'r' if (at(i + 1) == b'"' || at(i + 1) == b'#')
            && (i == 0 || !(at(i - 1).is_ascii_alphanumeric() || at(i - 1) == b'_')) =>
        {
            let mut j = i + 1;
            let mut hashes = 0;
            while at(j) == b'#' {
                hashes += 1;
                j += 1;
            }
            if at(j) != b'"' {
                return None;
            }
            j += 1;
            loop {
                if j >= s.len() {
                    return Some(j);
                }
                if s[j] == b'"' && (0..hashes).all(|h| at(j + 1 + h) == b'#') {
                    return Some(j + 1 + hashes);
                }
                j += 1;
            }
        }
        b'"' => {
            let mut j = i + 1;
            while j < s.len() && s[j] != b'"' {
                if s[j] == b'\\' {
                    j += 1;
                }
                j += 1;
            }
            Some(j + 1)
        }
        b'\'' => {
            // A char literal ('x', '\n', '{'); a lifetime ('a) is not.
            if at(i + 1) == b'\\' {
                let mut j = i + 2;
                while j < s.len() && s[j] != b'\'' {
                    j += 1;
                }
                Some(j + 1)
            } else if at(i + 2) == b'\'' {
                Some(i + 3)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// The index just past the bracket matching the one at `open`.
fn matching(s: &str, open: usize) -> usize {
    let b = s.as_bytes();
    let (o, c) = match b[open] {
        b'(' => (b'(', b')'),
        b'{' => (b'{', b'}'),
        b'[' => (b'[', b']'),
        other => panic!("not a bracket: {}", other as char),
    };
    let mut depth = 0i32;
    let mut i = open;
    while i < b.len() {
        if let Some(next) = skip_literal(b, i) {
            i = next;
            continue;
        }
        if b[i] == o {
            depth += 1;
        } else if b[i] == c {
            depth -= 1;
            if depth == 0 {
                return i + 1;
            }
        }
        i += 1;
    }
    b.len()
}

/// The first `ch` at or after `from` outside comments and literals.
fn find_code(s: &str, from: usize, ch: u8) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = from;
    while i < b.len() {
        if let Some(next) = skip_literal(b, i) {
            i = next;
            continue;
        }
        if b[i] == ch {
            return Some(i);
        }
        i += 1;
    }
    None
}

impl Source {
    fn load() -> Self {
        let root = src_root();
        let mut paths = Vec::new();
        walk_rs_files(&root, &mut paths);
        let fn_re = regex::Regex::new(r"(?m)^[ \t]*(?:pub(?:\([^)]*\))?[ \t]+)?(?:async[ \t]+)?fn[ \t]+([A-Za-z_][A-Za-z0-9_]*)")
            .unwrap();
        let mut files = HashMap::new();
        let mut fns = Vec::new();
        for p in paths {
            let rel = p
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let content = fs::read_to_string(&p).unwrap();
            let mut prev_end = 0;
            for m in fn_re.captures_iter(&content) {
                let whole = m.get(0).unwrap();
                let name = m[1].to_string();
                let start = whole.start();
                // Parameters, then the body (a declaration without one, as
                // in a trait, has none).
                let Some(params) = find_code(&content, whole.end(), b'(') else {
                    continue;
                };
                let params_end = matching(&content, params);
                let Some(open) = find_code(&content, params_end, b'{') else {
                    continue;
                };
                if content[params_end..open].contains(';') {
                    continue;
                }
                let end = matching(&content, open);
                let before = &content[prev_end.min(start)..start];
                let utoipa = before.rfind("#[utoipa::path(").map(|at| {
                    let from = prev_end.min(start) + at + "#[utoipa::path".len();
                    let close = matching(&content, from);
                    content[from..close].to_string()
                });
                fns.push(SourceFn {
                    file: rel.clone(),
                    name,
                    signature: content[start..open].to_string(),
                    body: content[open..end].to_string(),
                    utoipa,
                });
                prev_end = start.max(prev_end);
                // Nested fns are found by the regex too; `prev_end` only
                // bounds the attribute search.
                prev_end = prev_end.max(whole.end());
            }
            files.insert(rel, content);
        }
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, f) in fns.iter().enumerate() {
            by_name.entry(f.name.clone()).or_default().push(i);
        }
        Source {
            files,
            fns,
            by_name,
        }
    }

    /// Resolve a call path (`list_x`, `list_x::<U>`, `super::version_api::x`,
    /// `crate::function::api::functions_router`) seen in `from_file`.
    fn resolve(&self, path: &str, from_file: &str) -> Option<usize> {
        let path = path.split("::<").next().unwrap();
        let segments: Vec<&str> = path.split("::").collect();
        let name = *segments.last()?;
        let candidates = self.by_name.get(name)?;
        let modules: Vec<&str> = segments[..segments.len() - 1]
            .iter()
            .copied()
            .filter(|s| !matches!(*s, "super" | "crate" | "self"))
            .collect();
        let in_module = |i: &usize| {
            let file = &self.fns[*i].file;
            modules.iter().all(|m| {
                file.split('/')
                    .any(|seg| seg == *m || seg == format!("{m}.rs"))
            })
        };
        // Same file first, then the named module, then a unique match.
        if modules.is_empty() {
            if let Some(i) = candidates.iter().find(|i| self.fns[**i].file == from_file) {
                return Some(*i);
            }
            // A handler imported from a sibling module of the same directory.
            let dir = from_file.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
            let same_dir: Vec<&usize> = candidates
                .iter()
                .filter(|i| {
                    self.fns[**i]
                        .file
                        .rsplit_once('/')
                        .map(|(d, _)| d)
                        .unwrap_or("")
                        == dir
                })
                .collect();
            if same_dir.len() == 1 {
                return Some(*same_dir[0]);
            }
        } else {
            let hits: Vec<&usize> = candidates.iter().filter(|i| in_module(i)).collect();
            if hits.len() == 1 {
                return Some(*hits[0]);
            }
            if let Some(i) = hits.iter().find(|i| self.fns[***i].file == from_file) {
                return Some(**i);
            }
        }
        (candidates.len() == 1).then(|| candidates[0])
    }

    fn router_consts(&self) -> HashMap<String, String> {
        let re = regex::Regex::new(r#"const ([A-Z][A-Z0-9_]*): &str = "([^"]*)";"#).unwrap();
        self.files
            .values()
            .flat_map(|content| re.captures_iter(content))
            .map(|c| (c[1].to_string(), c[2].to_string()))
            .collect()
    }
}

// ─── Route extraction ──────────────────────────────────────────────────────

#[derive(Clone)]
struct Route {
    method: String,
    path: String,
    handler: usize,
}

/// Every `.name(` call at the top level of a builder chain in `text`, with
/// its argument text.
fn chain_calls<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
    let needle = format!(".{name}(");
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = text[from..].find(&needle) {
        let open = from + at + needle.len() - 1;
        let close = matching(text, open);
        out.push(&text[open + 1..close - 1]);
        from = close;
    }
    out
}

/// Split a call's argument text at its top-level commas.
fn split_args(args: &str) -> Vec<&str> {
    let b = args.as_bytes();
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    let mut i = 0;
    while i < b.len() {
        if let Some(next) = skip_literal(b, i) {
            i = next;
            continue;
        }
        match b[i] {
            b'(' | b'{' | b'[' | b'<' => depth += 1,
            b')' | b'}' | b']' | b'>' => depth -= 1,
            b',' if depth == 0 => {
                out.push(args[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if !args[start..].trim().is_empty() {
        out.push(args[start..].trim());
    }
    out
}

fn join(prefix: &str, path: &str) -> String {
    let joined = format!("{}{}", prefix.trim_end_matches('/'), path);
    let joined = if joined.len() > 1 {
        joined.trim_end_matches('/').to_string()
    } else {
        joined
    };
    if joined.is_empty() {
        "/".to_string()
    } else {
        joined
    }
}

/// The routes a router-building fn body mounts under `prefix`.
fn collect_routes(
    src: &Source,
    consts: &HashMap<String, String>,
    fn_idx: usize,
    prefix: &str,
    out: &mut Vec<Route>,
    problems: &mut Vec<String>,
    depth: usize,
) {
    assert!(depth < 8, "router recursion too deep");
    let f = &src.fns[fn_idx];
    let body = f.body.as_str();
    let method_re = regex::Regex::new(
        r"\b(get|post|put|patch|delete)\(\s*([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*)",
    )
    .unwrap();
    let path_arg = |arg: &str| -> Option<String> {
        let arg = arg.trim();
        if let Some(lit) = arg.strip_prefix('"').and_then(|a| a.strip_suffix('"')) {
            Some(lit.to_string())
        } else {
            consts.get(arg).cloned()
        }
    };

    for args in chain_calls(body, "route") {
        let parts = split_args(args);
        let Some(path) = parts.first().and_then(|p| path_arg(p)) else {
            problems.push(format!("{}::{}: unreadable .route({args})", f.file, f.name));
            continue;
        };
        let rest = parts[1..].join(",");
        let mut found = false;
        for m in method_re.captures_iter(&rest) {
            found = true;
            match src.resolve(&m[2], &f.file) {
                Some(h) => out.push(Route {
                    method: m[1].to_uppercase(),
                    path: join(prefix, &path),
                    handler: h,
                }),
                None => problems.push(format!(
                    "{}::{}: unresolved handler {}",
                    f.file, f.name, &m[2]
                )),
            }
        }
        if !found && !rest.contains("spa_handler") && !rest.contains("move ||") {
            problems.push(format!(
                "{}::{}: no method in .route({args})",
                f.file, f.name
            ));
        }
    }

    let attr_path_re = regex::Regex::new(r#"\bpath\s*=\s*"([^"]*)""#).unwrap();
    for args in chain_calls(body, "routes") {
        let inner = args.trim();
        let Some(list) = inner
            .strip_prefix("routes!(")
            .and_then(|l| l.strip_suffix(')'))
        else {
            problems.push(format!(
                "{}::{}: unreadable .routes({args})",
                f.file, f.name
            ));
            continue;
        };
        for handler in split_args(list) {
            let Some(h) = src.resolve(handler, &f.file) else {
                problems.push(format!(
                    "{}::{}: unresolved handler {handler}",
                    f.file, f.name
                ));
                continue;
            };
            let Some(attr) = src.fns[h].utoipa.as_deref() else {
                problems.push(format!(
                    "{}::{handler}: no #[utoipa::path]",
                    src.fns[h].file
                ));
                continue;
            };
            let attr = attr.trim_start_matches('(');
            let method = attr
                .split(|c: char| c == ',' || c.is_whitespace())
                .find(|t| !t.is_empty())
                .unwrap_or("")
                .to_uppercase();
            let path = attr_path_re.captures(attr).map(|c| c[1].to_string());
            match path {
                Some(p) => out.push(Route {
                    method,
                    path: join(prefix, &p),
                    handler: h,
                }),
                None => problems.push(format!(
                    "{}::{handler}: no path in #[utoipa::path]",
                    src.fns[h].file
                )),
            }
        }
    }

    // Nested and merged routers.
    let call_re = regex::Regex::new(r"([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*(?:router|routes))\s*(?:::<[^>]*>)?\(").unwrap();
    for (kind, args) in chain_calls(body, "nest")
        .into_iter()
        .map(|a| ("nest", a))
        .chain(chain_calls(body, "merge").into_iter().map(|a| ("merge", a)))
    {
        let parts = split_args(args);
        let (sub_prefix, expr) = if kind == "nest" {
            let Some(p) = parts.first().and_then(|p| path_arg(p)) else {
                problems.push(format!("{}::{}: unreadable .nest({args})", f.file, f.name));
                continue;
            };
            (join(prefix, &p), parts[1..].join(","))
        } else {
            (prefix.to_string(), parts.join(","))
        };
        let Some(m) = call_re.captures(&expr) else {
            // `.merge(router)` (a variable), Swagger, SPA services.
            continue;
        };
        match src.resolve(&m[1], &f.file) {
            Some(inner) if inner != fn_idx => {
                collect_routes(src, consts, inner, &sub_prefix, out, problems, depth + 1)
            }
            Some(_) => {}
            None => problems.push(format!(
                "{}::{}: unresolved router {}",
                f.file, f.name, &m[1]
            )),
        }
    }

    // A router fn that returns another's routes (`function_routes().with_state`).
    if chain_calls(body, "route").is_empty()
        && chain_calls(body, "routes").is_empty()
        && chain_calls(body, "nest").is_empty()
        && chain_calls(body, "merge").is_empty()
    {
        if let Some(m) = call_re.captures(body) {
            if let Some(inner) = src.resolve(&m[1], &f.file).filter(|i| *i != fn_idx) {
                collect_routes(src, consts, inner, prefix, out, problems, depth + 1);
            }
        }
    }
}

fn platform_routes(src: &Source) -> Vec<Route> {
    let consts = src.router_consts();
    let build = src
        .fns
        .iter()
        .position(|f| f.file == "router.rs" && f.name == "build")
        .expect("router.rs::build");
    let mut routes = Vec::new();
    let mut problems = Vec::new();
    collect_routes(src, &consts, build, "", &mut routes, &mut problems, 0);
    assert!(
        problems.is_empty(),
        "\n\nThe route scan could not read part of the router; teach it the new form:\n  - {}\n",
        problems.join("\n  - ")
    );
    routes
        .retain(|r| r.path == "/api" || r.path.starts_with("/api/") || r.path.starts_with("/bff/"));
    routes.sort_by(|a, b| (&a.path, &a.method).cmp(&(&b.path, &b.method)));
    routes.dedup_by(|a, b| a.path == b.path && a.method == b.method);
    routes
}

// ─── Checks ────────────────────────────────────────────────────────────────

/// The handler body plus the bodies of same-file fns it calls (one level),
/// so a check in a shared helper counts.
fn effective_body(src: &Source, h: usize) -> String {
    let f = &src.fns[h];
    let mut text = f.body.clone();
    let call_re = regex::Regex::new(r"\b([a-z_][a-z0-9_]*)\s*(?:::<[^>]*>)?\(").unwrap();
    let mut seen = HashSet::new();
    for m in call_re.captures_iter(&f.body) {
        let name = &m[1];
        if !seen.insert(name.to_string()) {
            continue;
        }
        if let Some(ids) = src.by_name.get(name) {
            for i in ids {
                if src.fns[*i].file == f.file && *i != h {
                    text.push_str(&src.fns[*i].body);
                }
            }
        }
    }
    text
}

fn authenticates(src: &Source, h: usize) -> bool {
    src.fns[h].signature.contains("Authenticated")
}

fn checks_authority(src: &Source, h: usize) -> bool {
    let body = effective_body(src, h);
    AUTH_CHECK_PATTERNS.iter().any(|p| body.contains(p))
}

fn gates_read(src: &Source, h: usize) -> bool {
    let body = effective_body(src, h);
    READ_CHECK_PATTERNS.iter().any(|p| body.contains(p))
}

fn key(r: &Route) -> String {
    format!("{} {}", r.method, r.path)
}

fn allowlisted(list: &[(&str, &str)], k: &str) -> bool {
    list.iter().any(|(route, _)| *route == k)
}

#[test]
fn the_route_scan_finds_the_platform_surface() {
    let src = Source::load();
    let routes = platform_routes(&src);
    let keys: HashSet<String> = routes.iter().map(key).collect();
    // Spot checks across both router styles and every mounting path.
    for expected in [
        "GET /api/clients",
        "POST /api/clients",
        "GET /api/applications/{id}",
        "GET /api/me/clients",
        "GET /bff/roles",
        "GET /bff/debug/events",
        "GET /api/functions",
        "POST /api/events/batch",
        "GET /api/public/platform",
        "GET /bff/developer/applications",
    ] {
        assert!(keys.contains(expected), "route scan missed {expected}");
    }
    assert!(routes.len() > 200, "only {} routes found", routes.len());

    if std::env::var("ROUTE_INVENTORY").is_ok() {
        let mut by_file: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for r in &routes {
            let h = &src.fns[r.handler];
            let checked = if r.method == "GET" {
                gates_read(&src, r.handler)
            } else {
                checks_authority(&src, r.handler)
            };
            let status = match (authenticates(&src, r.handler), checked) {
                (false, _) => "PUBLIC",
                (true, false) => "LOGIN",
                (true, true) => "PERM",
            };
            by_file.entry(h.file.clone()).or_default().push(format!(
                "{:6} {:6} {} -> {}",
                status, r.method, r.path, h.name
            ));
        }
        for (file, lines) in by_file {
            println!("## {file}");
            for l in lines {
                println!("{l}");
            }
        }
    }
}

#[test]
fn every_api_and_bff_route_authenticates() {
    let src = Source::load();
    let violations: Vec<String> = platform_routes(&src)
        .iter()
        .filter(|r| !authenticates(&src, r.handler) && !allowlisted(PUBLIC_ROUTES, &key(r)))
        .map(|r| {
            format!(
                "{} ({}::{})",
                key(r),
                src.fns[r.handler].file,
                src.fns[r.handler].name
            )
        })
        .collect();
    assert!(
        violations.is_empty(),
        "\n\nRoutes whose handler does not extract `Authenticated`. Authenticate the caller, \
         or add the route to PUBLIC_ROUTES with the reason:\n  - {}\n",
        violations.join("\n  - ")
    );
}

#[test]
fn every_api_and_bff_write_checks_authority() {
    let src = Source::load();
    let violations: Vec<String> = platform_routes(&src)
        .iter()
        .filter(|r| r.method != "GET")
        .filter(|r| authenticates(&src, r.handler))
        .filter(|r| !checks_authority(&src, r.handler))
        .filter(|r| !allowlisted(WRITES_WITHOUT_PERMISSION, &key(r)))
        .map(|r| {
            format!(
                "{} ({}::{})",
                key(r),
                src.fns[r.handler].file,
                src.fns[r.handler].name
            )
        })
        .collect();
    assert!(
        violations.is_empty(),
        "\n\nWrite routes that check no permission (CLAUDE.md: a write handler must call \
         require_anchor, require_permission or a can_* check). Add the check, or list the \
         route in WRITES_WITHOUT_PERMISSION with the reason:\n  - {}\n",
        violations.join("\n  - ")
    );
}

#[test]
fn every_api_and_bff_read_checks_authority() {
    let src = Source::load();
    let violations: Vec<String> = platform_routes(&src)
        .iter()
        .filter(|r| r.method == "GET")
        .filter(|r| authenticates(&src, r.handler))
        .filter(|r| !gates_read(&src, r.handler))
        .filter(|r| !allowlisted(READS_WITHOUT_PERMISSION, &key(r)))
        .map(|r| {
            format!(
                "{} ({}::{})",
                key(r),
                src.fns[r.handler].file,
                src.fns[r.handler].name
            )
        })
        .collect();
    assert!(
        violations.is_empty(),
        "\n\nRead routes that check no permission, where Go checks one. Add the `can_read_*` \
         check Go makes, or, when Go too lets any signed-in caller read it, list the route \
         in READS_WITHOUT_PERMISSION with the reason:\n  - {}\n",
        violations.join("\n  - ")
    );
}

#[test]
fn every_allowlisted_route_exists() {
    let src = Source::load();
    let keys: HashSet<String> = platform_routes(&src).iter().map(key).collect();
    let stale: Vec<&str> = PUBLIC_ROUTES
        .iter()
        .chain(WRITES_WITHOUT_PERMISSION)
        .chain(READS_WITHOUT_PERMISSION)
        .map(|(k, _)| *k)
        .filter(|k| !keys.contains(*k))
        .collect();
    assert!(
        stale.is_empty(),
        "\n\nAllowlisted routes that no longer exist; remove them:\n  - {}\n",
        stale.join("\n  - ")
    );
}
