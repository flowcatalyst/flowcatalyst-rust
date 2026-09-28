//! API Middleware: the platform's authenticator.
//!
//! The extractors ([`Authenticated`], [`OptionalAuth`], [`ClientIp`]) and
//! [`AuthLayer`] are fc-platform-core's, re-exported here at their old
//! paths; they authenticate through a [`TokenAuthenticator`]. [`AppState`]
//! is the platform's: the token service validates the credential and the
//! authorization service builds the caller's context.

pub use fc_platform_core::shared::middleware::*;

use crate::{auth::auth_service::AuthService, shared::authorization_service::AuthorizationService};
use fc_platform_core::shared::authorization_service::AuthContext;
use std::sync::Arc;

/// Application state containing shared services
#[derive(Clone)]
pub struct AppState {
    pub auth_service: Arc<AuthService>,
    pub authz_service: Arc<AuthorizationService>,
}

#[async_trait::async_trait]
impl TokenAuthenticator for AppState {
    async fn bearer_context(
        &self,
        token: &str,
    ) -> fc_platform_core::shared::error::Result<AuthContext> {
        let claims = self.auth_service.validate_token(token)?;
        self.authz_service.build_context(&claims).await
    }

    async fn session_context(
        &self,
        token: &str,
    ) -> fc_platform_core::shared::error::Result<Option<AuthContext>> {
        let Ok(session) = self.auth_service.validate_session_token(token) else {
            return Ok(None);
        };
        self.authz_service
            .session_context(&session.principal_id)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::auth_service::{AuthConfig, AuthService};
    use crate::principal::entity::Principal;
    use crate::role::repository::RoleRepository;
    use crate::shared::authorization_service::AuthorizationService;
    use axum::extract::FromRequestParts;
    use axum::http::{header, request::Parts, Request, StatusCode};
    use axum::response::IntoResponse;
    use fc_platform_core::principal_kind::{PrincipalType, UserScope};
    use std::sync::Arc;

    // ─── Test Helpers ──────────────────────────────────────────────────────

    /// Create a test AuthService with HS256 (no RSA keys needed)
    fn test_auth_service() -> AuthService {
        let config = AuthConfig {
            secret_key: "test-secret-key-for-middleware-tests-minimum-32-chars!!".to_string(),
            issuer: "flowcatalyst".to_string(),
            audience: "flowcatalyst".to_string(),
            access_token_expiry_secs: 3600,
            session_token_expiry_secs: 28800,
            refresh_token_expiry_secs: 86400,
            rsa_private_key: None,
            rsa_public_key: None,
            rsa_public_key_previous: None,
        };
        AuthService::new(config)
    }

    /// Create a test AuthorizationService with a lazily-connected pool.
    /// The DB won't be called for principals with empty roles
    /// (resolve_permissions short-circuits before querying).
    fn test_authz_service() -> AuthorizationService {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://invalid:invalid@localhost/invalid")
            .expect("lazy pool should not fail to construct");
        let role_repo = Arc::new(RoleRepository::new(&pool));
        AuthorizationService::new(role_repo)
    }

    /// Build an AppState for testing
    fn test_app_state() -> AppState {
        AppState {
            auth_service: Arc::new(test_auth_service()),
            authz_service: Arc::new(test_authz_service()),
        }
    }

    /// Build request Parts with the given headers and AppState in extensions
    fn make_parts_with_app_state(auth_header: Option<&str>, cookie_header: Option<&str>) -> Parts {
        let mut builder = Request::builder();
        if let Some(auth) = auth_header {
            builder = builder.header(header::AUTHORIZATION, auth);
        }
        if let Some(cookie) = cookie_header {
            builder = builder.header(header::COOKIE, cookie);
        }
        let req = builder.body(()).unwrap();
        let (mut parts, _body) = req.into_parts();
        parts
            .extensions
            .insert(Authenticator::new(test_app_state()));
        parts
    }

    /// Build request Parts without AppState in extensions
    fn make_parts_without_app_state(auth_header: Option<&str>) -> Parts {
        let mut builder = Request::builder();
        if let Some(auth) = auth_header {
            builder = builder.header(header::AUTHORIZATION, auth);
        }
        let req = builder.body(()).unwrap();
        let (parts, _body) = req.into_parts();
        parts
    }

    /// Generate a valid access token for a principal with no roles
    /// (so AuthorizationService won't hit the DB)
    fn generate_token_no_roles(auth_service: &AuthService) -> String {
        let principal = Principal::new_user("test@example.com", UserScope::Anchor);
        // Principal::new_user starts with empty roles, so resolve_permissions short-circuits
        auth_service.generate_access_token(&principal).unwrap()
    }

    /// Generate a valid access token for a client-scoped user with no roles
    fn generate_client_token(auth_service: &AuthService) -> String {
        let principal =
            Principal::new_user("user@client.com", UserScope::Client).with_client_id("client-abc");
        auth_service.generate_access_token(&principal).unwrap()
    }

    /// Generate a valid access token for a partner-scoped user with multiple clients
    fn generate_partner_token(auth_service: &AuthService) -> String {
        let mut principal = Principal::new_user("partner@example.com", UserScope::Partner);
        principal.grant_client_access("client-1");
        principal.grant_client_access("client-2");
        auth_service.generate_access_token(&principal).unwrap()
    }

    // ─── extract_session_cookie Tests ──────────────────────────────────────

    #[test]
    fn test_extract_session_cookie_present() {
        let req = Request::builder()
            .header(header::COOKIE, "fc_session=my-token-value; other=xyz")
            .body(())
            .unwrap();
        let (parts, _) = req.into_parts();

        let token = extract_session_cookie(&parts.headers);
        assert_eq!(token, Some("my-token-value".to_string()));
    }

    #[test]
    fn test_extract_session_cookie_only_cookie() {
        let req = Request::builder()
            .header(header::COOKIE, "fc_session=abc123")
            .body(())
            .unwrap();
        let (parts, _) = req.into_parts();

        let token = extract_session_cookie(&parts.headers);
        assert_eq!(token, Some("abc123".to_string()));
    }

    #[test]
    fn test_extract_session_cookie_missing() {
        let req = Request::builder()
            .header(header::COOKIE, "other_cookie=value; another=thing")
            .body(())
            .unwrap();
        let (parts, _) = req.into_parts();

        let token = extract_session_cookie(&parts.headers);
        assert_eq!(token, None);
    }

    #[test]
    fn test_extract_session_cookie_no_cookie_header() {
        let req = Request::builder().body(()).unwrap();
        let (parts, _) = req.into_parts();

        let token = extract_session_cookie(&parts.headers);
        assert_eq!(token, None);
    }

    #[test]
    fn test_extract_session_cookie_with_whitespace() {
        // Cookie pairs are trimmed, so leading whitespace around the pair is removed.
        // The value after "=" is taken as-is (no trim on the value portion).
        let req = Request::builder()
            .header(
                header::COOKIE,
                "other=x;  fc_session=spaced-token  ; more=y",
            )
            .body(())
            .unwrap();
        let (parts, _) = req.into_parts();

        let token = extract_session_cookie(&parts.headers);
        // "  fc_session=spaced-token  " → trimmed to "fc_session=spaced-token  "
        // starts_with("fc_session") → true, split('=').nth(1) → "spaced-token  "
        // BUT the value is "spaced-token  " only if trailing spaces are in the value.
        // Actually the cookie spec trims the pair, so value is "spaced-token".
        assert_eq!(token, Some("spaced-token".to_string()));
    }

    // ─── Authenticated Extractor: Token Extraction Tests ───────────────────

    #[tokio::test]
    async fn test_authenticated_valid_bearer_token() {
        let auth_service = test_auth_service();
        let token = generate_token_no_roles(&auth_service);
        let mut parts = make_parts_with_app_state(Some(&format!("Bearer {}", token)), None);

        let result = Authenticated::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());

        let auth = result.unwrap();
        assert_eq!(auth.0.email, Some("test@example.com".to_string()));
        assert_eq!(auth.0.scope, UserScope::Anchor);
        assert_eq!(auth.0.principal_type, PrincipalType::User);
    }

    #[tokio::test]
    async fn test_authenticated_missing_authorization_header() {
        let mut parts = make_parts_with_app_state(None, None);

        let result = Authenticated::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert_eq!(
            err.status,
            StatusCode::FORBIDDEN,
            "no credential: Go's 403 UNAUTHENTICATED"
        );
        assert!(err.message.contains("Missing authentication token"));
    }

    #[tokio::test]
    async fn test_authenticated_malformed_auth_header_no_bearer_prefix() {
        let mut parts = make_parts_with_app_state(Some("Basic dXNlcjpwYXNz"), None);

        let result = Authenticated::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert_eq!(
            err.status,
            StatusCode::FORBIDDEN,
            "no credential: Go's 403 UNAUTHENTICATED"
        );
        assert!(err.message.contains("Missing authentication token"));
    }

    #[tokio::test]
    async fn test_authenticated_empty_bearer_token() {
        // "Bearer " with empty value still passes extract_bearer_token (returns Some(""))
        // but validation should fail
        let mut parts = make_parts_with_app_state(Some("Bearer "), None);

        let result = Authenticated::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert_eq!(
            err.status,
            StatusCode::FORBIDDEN,
            "no credential: Go's 403 UNAUTHENTICATED"
        );
    }

    #[tokio::test]
    async fn test_authenticated_garbage_token() {
        let mut parts = make_parts_with_app_state(Some("Bearer not-a-valid-jwt"), None);

        let result = Authenticated::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert_eq!(err.status, StatusCode::UNAUTHORIZED);
    }

    // ─── Authenticated Extractor: Token Validation Tests ───────────────────

    #[tokio::test]
    async fn test_authenticated_expired_token() {
        // Create an auth service that generates already-expired tokens
        let config = AuthConfig {
            secret_key: "test-secret-key-for-middleware-tests-minimum-32-chars!!".to_string(),
            access_token_expiry_secs: -120, // Already expired (past default 60s leeway)
            ..AuthConfig::default()
        };
        let expired_auth_service = AuthService::new(config);
        let principal = Principal::new_user("test@example.com", UserScope::Anchor);
        let expired_token = expired_auth_service
            .generate_access_token(&principal)
            .unwrap();

        // Use the regular app state (which has a different auth service with normal expiry)
        // The token was signed with the same secret, so signature is valid, but it's expired
        let mut parts = make_parts_with_app_state(Some(&format!("Bearer {}", expired_token)), None);

        let result = Authenticated::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert_eq!(err.status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_authenticated_wrong_secret_token() {
        // Generate token with a different secret key
        let other_config = AuthConfig {
            secret_key: "completely-different-secret-key-minimum-32-characters!!".to_string(),
            ..AuthConfig::default()
        };
        let other_service = AuthService::new(other_config);
        let principal = Principal::new_user("test@example.com", UserScope::Anchor);
        let token = other_service.generate_access_token(&principal).unwrap();

        let mut parts = make_parts_with_app_state(Some(&format!("Bearer {}", token)), None);

        let result = Authenticated::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert_eq!(err.status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_authenticated_tampered_token() {
        let auth_service = test_auth_service();
        let mut token = generate_token_no_roles(&auth_service);
        token.push_str("tampered");

        let mut parts = make_parts_with_app_state(Some(&format!("Bearer {}", token)), None);

        let result = Authenticated::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert_eq!(err.status, StatusCode::UNAUTHORIZED);
    }

    // ─── Authenticated Extractor: Session Cookie Tests ─────────────────────

    /// Only a session token signs a browser in: an access token placed in
    /// the cookie is refused (it may be narrowed, delegated, or a service
    /// account's).
    #[tokio::test]
    async fn test_an_access_token_is_not_a_session_cookie() {
        let auth_service = test_auth_service();
        let token = generate_token_no_roles(&auth_service);

        let mut parts = make_parts_with_app_state(None, Some(&format!("fc_session={}", token)));

        let err = Authenticated::from_request_parts(&mut parts, &())
            .await
            .unwrap_err();
        assert_eq!(
            err.status,
            StatusCode::FORBIDDEN,
            "no credential: Go's 403 UNAUTHENTICATED"
        );
        assert!(err.message.contains("Session"), "{}", err.message);

        let mut parts = make_parts_with_app_state(None, Some(&format!("fc_session={}", token)));
        let opt = OptionalAuth::from_request_parts(&mut parts, &())
            .await
            .unwrap();
        assert!(opt.0.is_none());
    }

    /// A valid session token whose principal cannot be reloaded (here: no
    /// principal store) reads as logged out.
    #[tokio::test]
    async fn test_a_session_without_a_principal_is_logged_out() {
        let auth_service = test_auth_service();
        let principal = Principal::new_user("test@example.com", UserScope::Anchor);
        let token = auth_service.generate_session_token(&principal).unwrap();

        let mut parts = make_parts_with_app_state(None, Some(&format!("fc_session={}", token)));
        let err = Authenticated::from_request_parts(&mut parts, &())
            .await
            .unwrap_err();
        assert_eq!(
            err.status,
            StatusCode::FORBIDDEN,
            "no credential: Go's 403 UNAUTHENTICATED"
        );
    }

    #[tokio::test]
    async fn test_authenticated_bearer_takes_precedence_over_cookie() {
        let auth_service = test_auth_service();
        let bearer_token = generate_token_no_roles(&auth_service);

        // Cookie has a different (invalid) token — Bearer should take precedence and succeed
        let mut parts = make_parts_with_app_state(
            Some(&format!("Bearer {}", bearer_token)),
            Some("fc_session=invalid-cookie-token"),
        );

        let auth = Authenticated::from_request_parts(&mut parts, &())
            .await
            .unwrap();
        assert!(!auth.via_session_cookie());
    }

    /// Go `extractToken`: the Bearer scheme is case-insensitive.
    #[tokio::test]
    async fn test_bearer_scheme_is_case_insensitive() {
        let auth_service = test_auth_service();
        let token = generate_token_no_roles(&auth_service);
        let mut parts = make_parts_with_app_state(Some(&format!("bearer {}", token)), None);
        assert!(Authenticated::from_request_parts(&mut parts, &())
            .await
            .is_ok());
    }

    /// Go `extractToken`: an Authorization header of another scheme
    /// declares the caller's intent; the cookie is not consulted.
    #[tokio::test]
    async fn test_another_scheme_does_not_fall_back_to_the_cookie() {
        let mut parts =
            make_parts_with_app_state(Some("Basic dXNlcjpwYXNz"), Some("fc_session=anything"));
        let err = Authenticated::from_request_parts(&mut parts, &())
            .await
            .unwrap_err();
        assert!(err.message.contains("Missing authentication token"));
    }

    #[tokio::test]
    async fn test_authenticated_invalid_cookie_token() {
        let mut parts = make_parts_with_app_state(None, Some("fc_session=invalid-jwt-token"));

        let result = Authenticated::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert_eq!(
            err.status,
            StatusCode::FORBIDDEN,
            "no credential: Go's 403 UNAUTHENTICATED"
        );
    }

    #[test]
    fn test_session_cookie_name_must_match_exactly() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            header::COOKIE,
            axum::http::HeaderValue::from_static("fc_session_old=a; x=1"),
        );
        assert_eq!(extract_session_cookie(&headers), None);
        headers.insert(
            header::COOKIE,
            axum::http::HeaderValue::from_static("fc_session_old=a; fc_session=b"),
        );
        assert_eq!(extract_session_cookie(&headers), Some("b".to_string()));
    }

    // ─── Authenticated Extractor: Missing AppState ─────────────────────────

    #[tokio::test]
    async fn test_authenticated_missing_app_state() {
        let mut parts = make_parts_without_app_state(Some("Bearer some-token"));

        let result = Authenticated::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert_eq!(err.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(err.message.contains("Auth service not configured"));
    }

    // ─── Permission / Auth Context: Scope Tests ────────────────────────────

    #[tokio::test]
    async fn test_authenticated_anchor_user_context() {
        let auth_service = test_auth_service();
        let token = generate_token_no_roles(&auth_service);

        let mut parts = make_parts_with_app_state(Some(&format!("Bearer {}", token)), None);

        let auth = Authenticated::from_request_parts(&mut parts, &())
            .await
            .unwrap();

        assert!(auth.0.is_anchor());
        assert!(auth.0.can_access_client("any-client-id"));
        assert!(auth.0.can_access_client("another-client"));
        assert_eq!(auth.0.scope, UserScope::Anchor);
        assert!(auth.0.accessible_clients.contains(&"*".to_string()));
    }

    /// Go's middleware (shared/middleware/middleware.go:204-213): a bearer
    /// whose `scope` carries granted permissions authorizes exactly those,
    /// with no role lookup.
    #[tokio::test]
    async fn test_scope_claim_permissions_are_used_as_granted() {
        let auth_service = test_auth_service();
        let mut principal =
            Principal::new_user("svc@example.com", UserScope::Client).with_client_id("client-abc");
        // A role the (unreachable) DB would have to resolve: the scope
        // claim must make that lookup unnecessary.
        principal.assign_role("app:role");
        let token = auth_service
            .generate_access_token_with_scope(
                &principal,
                &["platform:iam:user:view".to_string()],
                None,
            )
            .unwrap();
        let mut parts = make_parts_with_app_state(Some(&format!("Bearer {}", token)), None);
        let auth = Authenticated::from_request_parts(&mut parts, &())
            .await
            .unwrap();
        assert!(auth.0.has_permission("platform:iam:user:view"));
        assert!(!auth.0.has_permission("platform:iam:user:create"));
    }

    /// Go refuses an identity-only (interactive-login) access token as an
    /// API credential (shared/middleware/middleware.go:185-194).
    #[tokio::test]
    async fn test_identity_token_is_not_an_api_credential() {
        let auth_service = test_auth_service();
        let principal = Principal::new_user("user@example.com", UserScope::Anchor);
        let token = auth_service
            .generate_identity_access_token(&principal, Some("oc_app"))
            .unwrap();
        let mut parts = make_parts_with_app_state(Some(&format!("Bearer {}", token)), None);
        let err = Authenticated::from_request_parts(&mut parts, &())
            .await
            .unwrap_err();
        assert_eq!(err.status, StatusCode::UNAUTHORIZED);
        assert!(err.message.contains("interactive login"), "{}", err.message);
    }

    #[tokio::test]
    async fn test_authenticated_client_user_context() {
        let auth_service = test_auth_service();
        let token = generate_client_token(&auth_service);

        let mut parts = make_parts_with_app_state(Some(&format!("Bearer {}", token)), None);

        let auth = Authenticated::from_request_parts(&mut parts, &())
            .await
            .unwrap();

        assert!(!auth.0.is_anchor());
        assert_eq!(auth.0.scope, UserScope::Client);
        assert!(auth.0.can_access_client("client-abc"));
        assert!(!auth.0.can_access_client("other-client"));
        assert_eq!(auth.0.email, Some("user@client.com".to_string()));
    }

    #[tokio::test]
    async fn test_authenticated_partner_user_multiple_clients() {
        let auth_service = test_auth_service();
        let token = generate_partner_token(&auth_service);

        let mut parts = make_parts_with_app_state(Some(&format!("Bearer {}", token)), None);

        let auth = Authenticated::from_request_parts(&mut parts, &())
            .await
            .unwrap();

        assert!(!auth.0.is_anchor());
        assert_eq!(auth.0.scope, UserScope::Partner);
        assert!(auth.0.can_access_client("client-1"));
        assert!(auth.0.can_access_client("client-2"));
        assert!(!auth.0.can_access_client("client-3"));
        assert_eq!(auth.0.email, Some("partner@example.com".to_string()));
    }

    // ─── OptionalAuth Extractor Tests ──────────────────────────────────────

    #[tokio::test]
    async fn test_optional_auth_valid_token() {
        let auth_service = test_auth_service();
        let token = generate_token_no_roles(&auth_service);

        let mut parts = make_parts_with_app_state(Some(&format!("Bearer {}", token)), None);

        let result = OptionalAuth::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());

        let opt_auth = result.unwrap();
        assert!(opt_auth.0.is_some());
        let ctx = opt_auth.0.unwrap();
        assert_eq!(ctx.scope, UserScope::Anchor);
        assert_eq!(ctx.email, Some("test@example.com".to_string()));
    }

    #[tokio::test]
    async fn test_optional_auth_missing_token_returns_none() {
        let mut parts = make_parts_with_app_state(None, None);

        let result = OptionalAuth::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());
        assert!(result.unwrap().0.is_none());
    }

    #[tokio::test]
    async fn test_optional_auth_invalid_token_returns_none() {
        let mut parts = make_parts_with_app_state(Some("Bearer invalid-token"), None);

        let result = OptionalAuth::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());
        assert!(result.unwrap().0.is_none());
    }

    #[tokio::test]
    async fn test_optional_auth_missing_app_state_returns_none() {
        let mut parts = make_parts_without_app_state(Some("Bearer some-token"));

        let result = OptionalAuth::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());
        assert!(result.unwrap().0.is_none());
    }

    // ─── ClientIp Extractor Tests ──────────────────────────────────────────

    #[tokio::test]
    async fn test_client_ip_reads_from_right_with_one_trusted_hop() {
        // Default hops=1 (ALB-only). The chain is `client, proxy_ip` after
        // ALB appends the real client IP. We want the appended-by-trusted
        // value, which is the second entry — but with hops=1 we pick the
        // entry one from the right, which is the original client.
        // Chain: "real_client, alb_added" → with 1 trusted hop, take the
        // entry the trusted proxy *saw before* appending = real_client.
        let req = Request::builder()
            .header("x-forwarded-for", "192.168.1.1, 10.0.0.1")
            .body(())
            .unwrap();
        let (mut parts, _) = req.into_parts();

        let result = ClientIp::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());
        // Last entry (rightmost) is what ALB appended — that's the trusted
        // value. 192.168.1.1 was supplied by whoever hit ALB; 10.0.0.1 is
        // ALB's own observation. With one trusted proxy in front of us, we
        // take entry-from-right = 1, which is the rightmost.
        assert_eq!(result.unwrap().0, Some("10.0.0.1".to_string()));
    }

    #[tokio::test]
    async fn test_client_ip_single_entry_chain() {
        // Just one entry in XFF — proxy added it, no client manipulation.
        let req = Request::builder()
            .header("x-forwarded-for", "203.0.113.42")
            .body(())
            .unwrap();
        let (mut parts, _) = req.into_parts();
        let result = ClientIp::from_request_parts(&mut parts, &()).await.unwrap();
        assert_eq!(result.0, Some("203.0.113.42".to_string()));
    }

    #[tokio::test]
    async fn test_client_ip_from_x_real_ip() {
        let req = Request::builder()
            .header("x-real-ip", "172.16.0.1")
            .body(())
            .unwrap();
        let (mut parts, _) = req.into_parts();

        let result = ClientIp::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, Some("172.16.0.1".to_string()));
    }

    #[tokio::test]
    async fn test_client_ip_no_headers() {
        let req = Request::builder().body(()).unwrap();
        let (mut parts, _) = req.into_parts();

        let result = ClientIp::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, None);
    }

    #[tokio::test]
    async fn test_client_ip_x_forwarded_for_takes_precedence() {
        let req = Request::builder()
            .header("x-forwarded-for", "1.2.3.4")
            .header("x-real-ip", "5.6.7.8")
            .body(())
            .unwrap();
        let (mut parts, _) = req.into_parts();

        let result = ClientIp::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, Some("1.2.3.4".to_string()));
    }

    #[test]
    fn extract_trusted_client_ip_attacker_prefix_is_ignored() {
        // Attacker spoofs the leftmost; ALB appends the real client. With
        // hops=1 we read the rightmost (ALB-added), so spoofing achieves
        // nothing.
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            axum::http::HeaderValue::from_static("evil.attacker, real.client"),
        );
        assert_eq!(
            extract_trusted_client_ip(&headers),
            Some("real.client".to_string()),
        );
    }

    #[test]
    fn extract_trusted_client_ip_falls_back_to_x_real_ip() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-real-ip", axum::http::HeaderValue::from_static("9.9.9.9"));
        assert_eq!(
            extract_trusted_client_ip(&headers),
            Some("9.9.9.9".to_string())
        );
    }

    #[test]
    fn extract_trusted_client_ip_returns_none_when_no_headers() {
        let headers = axum::http::HeaderMap::new();
        assert_eq!(extract_trusted_client_ip(&headers), None);
    }

    // ─── AuthError Response Tests ──────────────────────────────────────────

    #[test]
    fn test_auth_error_into_response() {
        let err = AuthError {
            status: StatusCode::UNAUTHORIZED,
            message: "Token expired".to_string(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn test_auth_error_internal_server_error() {
        let err = AuthError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "Auth service not configured".to_string(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
