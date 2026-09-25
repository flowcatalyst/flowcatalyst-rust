//! Auth middleware — populates `Option<Principal>` in request extensions.
//!
//! Bearer wins over cookie on the same request: an `Authorization` header is
//! explicit identification; we never silently downgrade to a stray browser
//! cookie. This matches the Fastify plugin's behavior.
//!
//! Token refresh: if the cookie session's access token is within 60s of
//! expiry and a refresh token is stored, we refresh in the background and
//! rewrite the cookie. Failed refresh → session cleared, request continues
//! unauthenticated.

use axum::{
    extract::Request,
    http::{HeaderMap, header::AUTHORIZATION},
    middleware::Next,
    response::Response,
};

use crate::auth::AuthContext;

use super::principal::{AuthMechanism, Principal};
use super::session::{PrincipalSnapshot, SessionPayload, SessionTokens};
use super::state::AuthState;

const REFRESH_LEEWAY_MS: i64 = 60_000;

/// Axum middleware that:
///   1. Reads `Authorization: Bearer …` and verifies via [`TokenValidator`].
///   2. Otherwise reads + decrypts the session cookie via [`SessionStore`].
///   3. Refreshes the access token if it's expiring soon and we have a
///      refresh token.
///   4. Inserts `Option<Principal>` into request extensions (always — even
///      when unauthenticated — so handlers can call `request.extensions().get::<Option<Principal>>()`).
///
/// Apply with `.layer(axum::middleware::from_fn_with_state(state, fc_auth_middleware))`
/// via [`super::router::auth_router`].
pub async fn fc_auth_middleware(
    axum::extract::State(state): axum::extract::State<AuthState>,
    mut request: Request,
    next: Next,
) -> Response {
    let principal = resolve_principal(&state, request.headers()).await;

    // Apply any cookie mutations (refresh / clear) to the response.
    let cookie_updates = std::mem::take(&mut *principal.cookie_updates.lock().expect("lock"));

    if let Some(p) = principal.principal {
        request.extensions_mut().insert(Some(p));
    } else {
        request.extensions_mut().insert::<Option<Principal>>(None);
    }

    let mut response = next.run(request).await;
    for (name, value) in cookie_updates {
        response.headers_mut().append(name, value);
    }
    response
}

struct ResolvedPrincipal {
    principal: Option<Principal>,
    cookie_updates: std::sync::Mutex<Vec<(axum::http::HeaderName, axum::http::HeaderValue)>>,
}

async fn resolve_principal(state: &AuthState, headers: &HeaderMap) -> ResolvedPrincipal {
    let cookie_updates = std::sync::Mutex::new(Vec::new());

    // 1. Bearer.
    if let Some(token) = read_bearer(headers) {
        match state.token_validator.validate(token).await {
            Ok(auth) => {
                return ResolvedPrincipal {
                    principal: Some(Principal::from_auth(
                        auth,
                        AuthMechanism::Bearer,
                        state.rbac.as_ref(),
                    )),
                    cookie_updates,
                };
            }
            Err(_) => {
                return ResolvedPrincipal {
                    principal: None,
                    cookie_updates,
                };
            }
        }
    }

    // 2. Session cookie.
    let Some(mut session) = state.session_store.read(headers).await else {
        return ResolvedPrincipal {
            principal: None,
            cookie_updates,
        };
    };

    // 3. Refresh token if needed.
    if let (Some(refresh_token), true) = (
        session.tokens.refresh_token.clone(),
        session.tokens.access_token_expires_at - now_ms() < REFRESH_LEEWAY_MS,
    ) {
        match state.oauth_client.refresh_token(&refresh_token).await {
            Ok(tr) => match state.token_validator.validate(&tr.access_token).await {
                Ok(auth) => {
                    session = make_session_from_refresh(&session, &tr, &auth, &state);
                    write_cookie(state, &session, &cookie_updates).await;
                }
                Err(_) => {
                    clear_cookie(state, &cookie_updates).await;
                    return ResolvedPrincipal {
                        principal: None,
                        cookie_updates,
                    };
                }
            },
            Err(_) => {
                clear_cookie(state, &cookie_updates).await;
                return ResolvedPrincipal {
                    principal: None,
                    cookie_updates,
                };
            }
        }
    }

    // Build a fresh AuthContext from the session snapshot so the principal
    // behaves identically to the Bearer path.
    let auth = build_auth_context_from_session(&session);
    ResolvedPrincipal {
        principal: Some(Principal::from_auth(
            auth,
            AuthMechanism::Session,
            state.rbac.as_ref(),
        )),
        cookie_updates,
    }
}

fn read_bearer(headers: &HeaderMap) -> Option<&str> {
    let raw = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let lower = raw.trim();
    let mut iter = lower.splitn(2, ' ');
    if iter.next()?.eq_ignore_ascii_case("bearer") {
        iter.next().map(str::trim)
    } else {
        None
    }
}

fn make_session_from_refresh(
    old: &SessionPayload,
    tr: &crate::auth::oauth::TokenResponse,
    auth: &AuthContext,
    state: &AuthState,
) -> SessionPayload {
    let expires_at = now_ms() + tr.expires_in * 1000;
    SessionPayload {
        principal: PrincipalSnapshot {
            id: auth.principal_id().to_string(),
            principal_type: auth.claims.principal_type.clone(),
            scope: auth.claims.tier().to_string(),
            name: auth.name().to_string(),
            email: auth.email().map(str::to_string),
            clients: auth.claims.clients.clone(),
            roles: auth.claims.roles.clone(),
            applications: auth.claims.applications.clone(),
        },
        tokens: SessionTokens {
            access_token: tr.access_token.clone(),
            access_token_expires_at: expires_at,
            refresh_token: tr
                .refresh_token
                .clone()
                .or_else(|| old.tokens.refresh_token.clone()),
        },
        session_data: old.session_data.clone(),
        expires_at: now_ms() + state.session_max_age_ms,
        mechanism: AuthMechanism::Session,
    }
}

fn build_auth_context_from_session(session: &SessionPayload) -> AuthContext {
    let claims = crate::auth::AccessTokenClaims {
        sub: session.principal.id.clone(),
        iss: String::new(),
        aud: String::new(),
        exp: session.tokens.access_token_expires_at / 1000,
        iat: 0,
        nbf: 0,
        jti: String::new(),
        principal_type: session.principal.principal_type.clone(),
        scope: session.principal.scope.clone(),
        email: session.principal.email.clone(),
        name: session.principal.name.clone(),
        clients: session.principal.clients.clone(),
        roles: session.principal.roles.clone(),
        applications: session.principal.applications.clone(),
        tier: Some(session.principal.scope.clone()),
        ..Default::default()
    };
    AuthContext::new(claims, session.tokens.access_token.clone())
}

async fn write_cookie(
    state: &AuthState,
    session: &SessionPayload,
    out: &std::sync::Mutex<Vec<(axum::http::HeaderName, axum::http::HeaderValue)>>,
) {
    let mut h = HeaderMap::new();
    if state.session_store.write(session, &mut h).await.is_ok() {
        for (k, v) in h.iter() {
            out.lock().expect("lock").push((k.clone(), v.clone()));
        }
    }
}

async fn clear_cookie(
    state: &AuthState,
    out: &std::sync::Mutex<Vec<(axum::http::HeaderName, axum::http::HeaderValue)>>,
) {
    let mut h = HeaderMap::new();
    if state.session_store.clear(&mut h).await.is_ok() {
        for (k, v) in h.iter() {
            out.lock().expect("lock").push((k.clone(), v.clone()));
        }
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
