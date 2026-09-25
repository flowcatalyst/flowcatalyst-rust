//! Axum handlers for the OIDC authorization-code/PKCE flow.
//!
//!   GET /auth/login    — generate PKCE + state, stash in a short-lived
//!                        encrypted cookie, 302 to the platform's
//!                        `/oauth/authorize`.
//!   GET /auth/callback — verify state, exchange code, build session, set
//!                        the session cookie, 302 to `returnTo`.
//!   POST /auth/logout  — clear the session cookie. Optionally redirect via
//!                        `redirectTo` form/query param.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header::SET_COOKIE},
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};

use crate::auth::PkceChallenge;

use super::crypto::SessionCrypto;
use super::principal::AuthMechanism;
use super::session::{PrincipalSnapshot, SessionPayload, SessionTokens};
use super::state::AuthState;

const STATE_COOKIE: &str = "fc_oauth_state";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateBag {
    pub state: String,
    pub code_verifier: String,
    pub return_to: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginQuery {
    #[serde(default)]
    #[serde(alias = "returnTo")]
    pub return_to: Option<String>,
}

/// Shared state used by the OIDC handlers — wraps [`AuthState`] plus a
/// short-lived crypto for the state cookie.
#[derive(Clone)]
pub struct OidcState {
    pub auth: AuthState,
    pub state_crypto: Arc<SessionCrypto>,
}

pub async fn login_handler(
    State(state): State<OidcState>,
    Query(q): Query<LoginQuery>,
) -> Response {
    let return_to = sanitize_return_to(q.return_to.as_deref());
    let pkce = PkceChallenge::generate();
    let oauth_state_value = random_b64u(16);
    let bag = StateBag {
        state: oauth_state_value.clone(),
        code_verifier: pkce.code_verifier.clone(),
        return_to,
    };

    let (authorize_url, _params) = state.auth.oauth_client.authorize_url();
    // Inject our own state + code_challenge by appending. OAuthClient already
    // builds these, but we override `state` so we can verify it later.
    let url = override_state(&authorize_url, &oauth_state_value, &pkce.code_challenge);

    let mut headers = HeaderMap::new();
    let envelope = match serde_json::to_vec(&bag).and_then(|j| Ok(state.state_crypto.encrypt(&j))) {
        Ok(Ok(env)) => env,
        _ => return (StatusCode::INTERNAL_SERVER_ERROR, "state encrypt failed").into_response(),
    };

    let cookie = Cookie::build((STATE_COOKIE.to_owned(), envelope))
        .path(state.auth.routes.callback.clone())
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .max_age(time::Duration::seconds(600))
        .build();
    if let Ok(v) = cookie.to_string().parse() {
        headers.append(SET_COOKIE, v);
    }

    let mut resp = Redirect::to(&url).into_response();
    resp.headers_mut().extend(headers);
    resp
}

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
}

pub async fn callback_handler(
    State(state): State<OidcState>,
    headers: HeaderMap,
    Query(q): Query<CallbackQuery>,
) -> Response {
    let (Some(code), Some(state_param)) = (q.code, q.state) else {
        return (StatusCode::BAD_REQUEST, "missing code/state").into_response();
    };

    let bag = match read_state_cookie(&headers, &state.state_crypto) {
        Some(b) => b,
        None => return (StatusCode::BAD_REQUEST, "invalid oauth state").into_response(),
    };
    if bag.state != state_param {
        return (StatusCode::BAD_REQUEST, "invalid oauth state").into_response();
    }

    let token_response = match state
        .auth
        .oauth_client
        .exchange_code(&code, &bag.code_verifier)
        .await
    {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("token exchange failed: {e}"),
            )
                .into_response();
        }
    };

    let auth = match state
        .auth
        .token_validator
        .validate(&token_response.access_token)
        .await
    {
        Ok(a) => a,
        Err(e) => return (StatusCode::BAD_GATEWAY, format!("verify failed: {e}")).into_response(),
    };

    let expires_in = token_response.expires_in;
    let session = SessionPayload {
        principal: PrincipalSnapshot {
            id: auth.principal_id().to_string(),
            principal_type: auth.claims.principal_type.clone(),
            tier: auth.claims.tier.clone(),
            scope: auth.claims.scope.clone(),
            name: auth.name().to_string(),
            email: auth.email().map(str::to_string),
            clients: auth.claims.clients.clone(),
            roles: auth.claims.roles.clone(),
            applications: auth.claims.applications.clone(),
            all_applications: auth.claims.all_applications,
        },
        tokens: SessionTokens {
            access_token: token_response.access_token.clone(),
            access_token_expires_at: now_ms() + expires_in * 1000,
            refresh_token: token_response.refresh_token.clone(),
        },
        session_data: serde_json::json!({}),
        expires_at: now_ms() + state.auth.session_max_age_ms,
        mechanism: AuthMechanism::Session,
    };

    let mut out_headers = HeaderMap::new();
    // Clear the state cookie (consumed).
    clear_state_cookie(&mut out_headers, &state.auth.routes.callback);
    if let Err(e) = state
        .auth
        .session_store
        .write(&session, &mut out_headers)
        .await
    {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("session write: {e}")).into_response();
    }

    let mut resp = Redirect::to(&bag.return_to).into_response();
    resp.headers_mut().extend(out_headers);
    resp
}

#[derive(Debug, Deserialize)]
pub struct LogoutBody {
    #[serde(default)]
    #[serde(alias = "redirectTo")]
    pub redirect_to: Option<String>,
}

pub async fn logout_handler(
    State(state): State<OidcState>,
    headers: HeaderMap,
    body: Option<axum::Json<LogoutBody>>,
) -> Response {
    let mut out = HeaderMap::new();
    let _ = state.auth.session_store.clear(&mut out).await;
    let _ = headers; // headers reserved for future RP-initiated logout signalling
    let redirect = body.and_then(|axum::Json(b)| b.redirect_to);
    if let Some(loc) = redirect {
        let mut resp = Redirect::to(&loc).into_response();
        resp.headers_mut().extend(out);
        resp
    } else {
        let mut resp = (StatusCode::NO_CONTENT, ()).into_response();
        resp.headers_mut().extend(out);
        resp
    }
}

fn sanitize_return_to(raw: Option<&str>) -> String {
    let Some(v) = raw else {
        return "/".to_string();
    };
    let decoded = urlencoding::decode(v).map(|c| c.into_owned()).unwrap_or_default();
    if decoded.starts_with('/') && !decoded.starts_with("//") {
        decoded
    } else {
        "/".to_string()
    }
}

fn random_b64u(bytes: usize) -> String {
    use rand::RngCore;
    let mut buf = vec![0u8; bytes];
    rand::rng().fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

fn read_state_cookie(headers: &HeaderMap, crypto: &SessionCrypto) -> Option<StateBag> {
    let jar = parse_jar(headers);
    let envelope = jar.get(STATE_COOKIE)?.value().to_owned();
    let bytes = crypto.decrypt(&envelope)?;
    serde_json::from_slice(&bytes).ok()
}

fn clear_state_cookie(headers: &mut HeaderMap, path: &str) {
    let cookie = Cookie::build((STATE_COOKIE.to_owned(), String::new()))
        .path(path.to_owned())
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .max_age(time::Duration::seconds(0))
        .build();
    if let Ok(v) = cookie.to_string().parse() {
        headers.append(SET_COOKIE, v);
    }
}

fn parse_jar(headers: &HeaderMap) -> CookieJar {
    CookieJar::from_headers(headers)
}

/// The platform's `OAuthClient::authorize_url()` already builds a URL with
/// its own randomly-generated `state` + `code_challenge`. We re-issue both
/// so we control the values and can verify state on callback.
fn override_state(url: &str, state_value: &str, code_challenge: &str) -> String {
    let mut params: HashMap<String, String> = HashMap::new();
    if let Some(query_start) = url.find('?') {
        let base = &url[..query_start];
        let query = &url[query_start + 1..];
        for pair in query.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                params.insert(
                    urlencoding::decode(k).map(|c| c.into_owned()).unwrap_or_default(),
                    urlencoding::decode(v).map(|c| c.into_owned()).unwrap_or_default(),
                );
            }
        }
        params.insert("state".into(), state_value.into());
        params.insert("code_challenge".into(), code_challenge.into());
        params.insert("code_challenge_method".into(), "S256".into());
        let q = params
            .iter()
            .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join("&");
        format!("{base}?{q}")
    } else {
        format!(
            "{url}?state={}&code_challenge={}&code_challenge_method=S256",
            urlencoding::encode(state_value),
            urlencoding::encode(code_challenge)
        )
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

