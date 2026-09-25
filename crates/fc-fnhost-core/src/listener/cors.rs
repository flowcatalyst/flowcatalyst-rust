//! CORS on both listeners, per endpoint (Java `fnhost/http/CorsPolicy.java`,
//! spec `function-public-routes.md` §4). Only an endpoint that declares
//! `cors` is touched. The host answers a genuine preflight itself (no
//! invoke, no permit, no auth) and, on an actual request, its own
//! `-Allow-Origin` / `-Allow-Credentials` / `Vary` replace whatever the
//! function set.

use fc_function_abi::MultiMap;
use fc_function_model::{Cors, Endpoint};
use http::HeaderMap;

use super::answer::HttpAnswer;

const MANAGED: [&str; 3] = [
    "access-control-allow-origin",
    "access-control-allow-credentials",
    "vary",
];

fn first(headers: &HeaderMap, name: &str) -> Option<String> {
    headers.get(name).map(|v| super::latin1(v.as_bytes()))
}

/// `OPTIONS` + `Origin` + `Access-Control-Request-Method`, all three.
pub(crate) fn is_preflight(method: &str, headers: &HeaderMap) -> bool {
    method.eq_ignore_ascii_case("OPTIONS")
        && headers.contains_key("origin")
        && headers.contains_key("access-control-request-method")
}

/// The `204` answering a preflight: CORS headers only when both the origin
/// and the requested method are allowed; otherwise none at all (the server
/// does not reveal policy by status).
pub(crate) fn preflight(endpoint: &Endpoint, cors: &Cors, headers: &HeaderMap) -> HttpAnswer {
    let origin = first(headers, "origin");
    let requested = first(headers, "access-control-request-method");
    let allowed_methods = effective_methods(endpoint, cors, requested.as_deref());
    let method_ok = requested
        .as_deref()
        .is_some_and(|m| allowed_methods.contains(&m.trim().to_uppercase()));
    let Some(origin) = origin.filter(|o| origin_allowed(cors, Some(o)) && method_ok) else {
        return HttpAnswer::new(204, MultiMap::new(), Vec::new());
    };
    let wildcard = cors.origins.iter().any(|o| o == "*");
    let mut out = MultiMap::new();
    out.insert(
        "Access-Control-Allow-Origin".into(),
        vec![if wildcard { "*".to_owned() } else { origin }],
    );
    out.insert(
        "Access-Control-Allow-Methods".into(),
        vec![allowed_methods.join(", ")],
    );
    let allow_headers = allowed_headers(cors, first(headers, "access-control-request-headers"));
    if !allow_headers.is_empty() {
        out.insert(
            "Access-Control-Allow-Headers".into(),
            vec![allow_headers.join(", ")],
        );
    }
    if cors.allow_credentials {
        out.insert(
            "Access-Control-Allow-Credentials".into(),
            vec!["true".into()],
        );
    }
    out.insert("Access-Control-Max-Age".into(), vec!["600".into()]);
    if !wildcard {
        out.insert("Vary".into(), vec!["Origin".into()]);
    }
    HttpAnswer::new(204, out, Vec::new())
}

/// The actual-request rule, applied to whatever was answered (the
/// function's response or a host error). A no-op without `cors`.
pub(crate) fn apply_to_actual_response(
    endpoint: &Endpoint,
    origin: Option<&str>,
    mut answer: HttpAnswer,
) -> HttpAnswer {
    let Some(cors) = &endpoint.cors else {
        return answer;
    };
    answer
        .headers
        .retain(|name, _| !MANAGED.contains(&name.to_ascii_lowercase().as_str()));
    if let Some(origin) = origin.filter(|o| origin_allowed(cors, Some(o))) {
        let wildcard = cors.origins.iter().any(|o| o == "*");
        answer.headers.insert(
            "Access-Control-Allow-Origin".into(),
            vec![if wildcard {
                "*".to_owned()
            } else {
                origin.to_owned()
            }],
        );
        if cors.allow_credentials {
            answer.headers.insert(
                "Access-Control-Allow-Credentials".into(),
                vec!["true".into()],
            );
        }
        if !wildcard {
            answer.headers.insert("Vary".into(), vec!["Origin".into()]);
        }
    }
    answer
}

fn origin_allowed(cors: &Cors, origin: Option<&str>) -> bool {
    let Some(origin) = origin else {
        return false;
    };
    if cors.origins.iter().any(|o| o == "*") {
        return true;
    }
    let requested = parse_origin(origin);
    requested.is_some() && cors.origins.iter().any(|c| parse_origin(c) == requested)
}

/// Scheme and host (case-insensitive) and the port as written (`-1` when
/// absent), as `java.net.URI` reads them; `None` when there is no scheme or
/// host.
fn parse_origin(raw: &str) -> Option<(String, String, i32)> {
    let (scheme, rest) = raw.split_once("://")?;
    if scheme.is_empty()
        || !scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        || !scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, hp)| hp);
    let (host, port) = if let Some(bracketed) = host_port.strip_prefix('[') {
        let (host, after) = bracketed.split_once(']')?;
        (format!("[{host}]"), after.strip_prefix(':'))
    } else {
        match host_port.rsplit_once(':') {
            Some((host, port)) => (host.to_owned(), Some(port)),
            None => (host_port.to_owned(), None),
        }
    };
    if host.is_empty() {
        return None;
    }
    let port = match port {
        None | Some("") => -1,
        Some(p) if p.bytes().all(|b| b.is_ascii_digit()) => p.parse().ok()?,
        Some(_) => return None,
    };
    Some((scheme.to_lowercase(), host.to_lowercase(), port))
}

/// `cors.methods` ∪ the endpoint's own (webhook ⇒ `POST`); when the
/// endpoint takes every method, the requested one is what is echoed.
fn effective_methods(endpoint: &Endpoint, cors: &Cors, requested: Option<&str>) -> Vec<String> {
    let mut methods: Vec<String> = Vec::new();
    let mut add = |m: String| {
        if !methods.contains(&m) {
            methods.push(m);
        }
    };
    for m in &cors.methods {
        add(m.trim().to_uppercase());
    }
    let declared = endpoint.effective_methods();
    if declared.is_empty() {
        if let Some(requested) = requested.filter(|r| !crate::java::is_blank(r)) {
            add(requested.trim().to_uppercase());
        }
    } else {
        for m in declared {
            add(m.as_str().to_owned());
        }
    }
    methods
}

/// `cors.headers` ∩ requested, case-insensitive, in the manifest's spelling.
fn allowed_headers(cors: &Cors, requested: Option<String>) -> Vec<String> {
    let Some(requested) = requested.filter(|r| !crate::java::is_blank(r)) else {
        return Vec::new();
    };
    let requested: Vec<&str> = requested
        .split(',')
        .map(str::trim)
        .filter(|h| !h.is_empty())
        .collect();
    cors.headers
        .iter()
        .filter(|configured| requested.iter().any(|r| r.eq_ignore_ascii_case(configured)))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fc_function_model::{EndpointAuth, HttpMethod, RoutePattern};

    fn endpoint(cors: Cors, methods: Vec<HttpMethod>) -> Endpoint {
        Endpoint {
            path: RoutePattern::parse("/api/*").unwrap(),
            auth: EndpointAuth::None,
            methods,
            cors: Some(cors),
            max_body_bytes: 10,
            timeout_ms: 10,
        }
    }

    fn cors(origins: &[&str], methods: &[&str], headers: &[&str], credentials: bool) -> Cors {
        let own = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect();
        Cors {
            origins: own(origins),
            methods: own(methods),
            headers: own(headers),
            allow_credentials: credentials,
        }
    }

    fn request(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (k, v) in pairs {
            headers.append(
                http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                v.parse().unwrap(),
            );
        }
        headers
    }

    #[test]
    fn origins_compare_by_scheme_host_and_written_port() {
        assert_eq!(
            parse_origin("HTTPS://App.Acme.com"),
            Some(("https".into(), "app.acme.com".into(), -1))
        );
        assert_eq!(parse_origin("https://a.com:8443/x").unwrap().2, 8443);
        assert_ne!(
            parse_origin("https://a.com:443"),
            parse_origin("https://a.com")
        );
        assert_eq!(parse_origin("null"), None);
        assert_eq!(parse_origin("https://"), None);
    }

    #[test]
    fn preflight_with_every_header() {
        let endpoint = endpoint(
            cors(&["https://app.acme.com"], &["GET"], &["X-Custom"], true),
            vec![HttpMethod::Post],
        );
        let answer = preflight(
            &endpoint,
            endpoint.cors.as_ref().unwrap(),
            &request(&[
                ("origin", "https://app.acme.com"),
                ("access-control-request-method", "post"),
                ("access-control-request-headers", "x-custom, x-other"),
            ]),
        );
        assert_eq!(answer.status, 204);
        let h = |n: &str| answer.headers.get(n).cloned().unwrap_or_default();
        assert_eq!(h("Access-Control-Allow-Origin"), ["https://app.acme.com"]);
        assert_eq!(h("Access-Control-Allow-Methods"), ["GET, POST"]);
        assert_eq!(h("Access-Control-Allow-Headers"), ["X-Custom"]);
        assert_eq!(h("Access-Control-Allow-Credentials"), ["true"]);
        assert_eq!(h("Access-Control-Max-Age"), ["600"]);
        assert_eq!(h("Vary"), ["Origin"]);
    }

    #[test]
    fn a_disallowed_preflight_reveals_nothing() {
        let endpoint = endpoint(
            cors(&["https://app.acme.com"], &["GET"], &[], false),
            vec![],
        );
        for headers in [
            request(&[
                ("origin", "https://evil.test"),
                ("access-control-request-method", "GET"),
            ]),
            request(&[
                ("origin", "https://app.acme.com"),
                ("access-control-request-method", " "),
            ]),
        ] {
            let answer = preflight(&endpoint, endpoint.cors.as_ref().unwrap(), &headers);
            assert_eq!(answer.status, 204);
            assert!(answer.headers.is_empty(), "{:?}", answer.headers);
        }
        // an endpoint taking every method echoes the requested one
        let answer = preflight(
            &endpoint,
            endpoint.cors.as_ref().unwrap(),
            &request(&[
                ("origin", "https://app.acme.com"),
                ("access-control-request-method", "delete"),
            ]),
        );
        assert_eq!(
            answer.headers["Access-Control-Allow-Methods"],
            ["GET, DELETE"]
        );
    }

    #[test]
    fn the_actual_response_is_the_hosts_alone() {
        let endpoint = endpoint(cors(&["*"], &[], &[], false), vec![]);
        let mut function = MultiMap::new();
        function.insert(
            "access-control-allow-origin".into(),
            vec!["https://x".into()],
        );
        function.insert("VARY".into(), vec!["Accept".into()]);
        function.insert("X-Own".into(), vec!["kept".into()]);
        let answer = apply_to_actual_response(
            &endpoint,
            Some("https://anyone"),
            HttpAnswer::new(200, function.clone(), Vec::new()),
        );
        assert_eq!(answer.headers["Access-Control-Allow-Origin"], ["*"]);
        assert!(!answer.headers.contains_key("Vary") && !answer.headers.contains_key("VARY"));
        assert_eq!(answer.headers["X-Own"], ["kept"]);
        let no_origin =
            apply_to_actual_response(&endpoint, None, HttpAnswer::new(200, function, Vec::new()));
        assert_eq!(no_origin.headers.keys().collect::<Vec<_>>(), ["X-Own"]);
    }
}
