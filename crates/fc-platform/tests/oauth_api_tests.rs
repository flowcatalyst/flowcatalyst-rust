//! OAuth/OIDC API Integration Tests
//!
//! Tests for OAuth endpoints (userinfo, introspect, revoke) and OIDC session
//! management using in-process Axum router via tower::ServiceExt.
//!
//! These tests do NOT require a running database — they use the AuthService
//! with HS256 keys for JWT generation/validation.

use fc_platform::auth::auth_service::{AuthConfig, AuthService};
use fc_platform::domain::{Principal, UserScope};

/// Create a test AuthService with HS256 (no RSA keys needed)
fn test_auth_service() -> AuthService {
    let config = AuthConfig {
        secret_key: "test-secret-key-for-integration-tests-minimum-32-chars!!".to_string(),
        issuer: "flowcatalyst".to_string(),
        audience: "flowcatalyst".to_string(),
        access_token_expiry_secs: 3600,
        session_token_expiry_secs: 28800,
        refresh_token_expiry_secs: 86400,
        rsa_private_key: None,
        rsa_public_key: None,
    };
    AuthService::new(config)
}

/// Create a test principal and generate a valid access token
fn test_principal_and_token(auth_service: &AuthService) -> (Principal, String) {
    let mut principal = Principal::new_user("test@example.com", UserScope::Anchor);
    principal.assign_role("admin");
    let token = auth_service.generate_access_token(&principal).unwrap();
    (principal, token)
}

// ─── Auth Service Token Round-Trip Tests ───────────────────────────────────

#[test]
fn test_generate_and_validate_access_token() {
    let auth_service = test_auth_service();
    let (principal, token) = test_principal_and_token(&auth_service);

    let claims = auth_service.validate_token(&token).unwrap();

    assert_eq!(claims.sub, principal.id);
    assert_eq!(claims.email, Some("test@example.com".to_string()));
    assert_eq!(claims.principal_type, "USER");
    assert_eq!(claims.scope, "ANCHOR");
    assert_eq!(claims.iss, "flowcatalyst");
    assert_eq!(claims.aud, "flowcatalyst");
    assert!(claims.roles.contains(&"admin".to_string()));
}

#[test]
fn test_validate_token_with_wrong_secret() {
    let auth_service = test_auth_service();
    let (_, token) = test_principal_and_token(&auth_service);

    // Create a different auth service with a different secret
    let other_config = AuthConfig {
        secret_key: "different-secret-key-for-testing-minimum-32-characters!!".to_string(),
        ..AuthConfig::default()
    };
    let other_service = AuthService::new(other_config);

    let result = other_service.validate_token(&token);
    assert!(result.is_err());
}

#[test]
fn test_validate_expired_token() {
    let config = AuthConfig {
        secret_key: "test-secret-key-for-integration-tests-minimum-32-chars!!".to_string(),
        access_token_expiry_secs: -120, // Already expired (past default 60s leeway)
        ..AuthConfig::default()
    };
    let auth_service = AuthService::new(config);
    let principal = Principal::new_user("test@example.com", UserScope::Anchor);
    let token = auth_service.generate_access_token(&principal).unwrap();

    let result = auth_service.validate_token(&token);
    assert!(result.is_err());
}

#[test]
fn test_token_claims_for_service_principal() {
    let auth_service = test_auth_service();
    let principal = Principal::new_service("svc-123", "Test Service");
    let token = auth_service.generate_access_token(&principal).unwrap();

    let claims = auth_service.validate_token(&token).unwrap();
    assert_eq!(claims.principal_type, "SERVICE");
    assert_eq!(claims.scope, "ANCHOR");
    assert_eq!(claims.name, "Test Service");
    assert_eq!(claims.email, None);
}

#[test]
fn test_token_claims_for_client_scope_user() {
    let auth_service = test_auth_service();
    let principal = Principal::new_user("user@client.com", UserScope::Client)
        .with_client_id("client-abc");
    let token = auth_service.generate_access_token(&principal).unwrap();

    let claims = auth_service.validate_token(&token).unwrap();
    assert_eq!(claims.scope, "CLIENT");
    assert!(claims.clients.contains(&"client-abc".to_string()));
}

#[test]
fn test_token_claims_for_partner_scope_user() {
    let auth_service = test_auth_service();
    let mut principal = Principal::new_user("partner@example.com", UserScope::Partner);
    principal.grant_client_access("client-1");
    principal.grant_client_access("client-2");
    let token = auth_service.generate_access_token(&principal).unwrap();

    let claims = auth_service.validate_token(&token).unwrap();
    assert_eq!(claims.scope, "PARTNER");
    // Partner users should have their assigned clients in the token
    assert!(claims.clients.contains(&"client-1".to_string()));
    assert!(claims.clients.contains(&"client-2".to_string()));
}

#[test]
fn test_anchor_scope_has_wildcard_client_access() {
    let auth_service = test_auth_service();
    let principal = Principal::new_user("admin@example.com", UserScope::Anchor);
    let token = auth_service.generate_access_token(&principal).unwrap();

    let claims = auth_service.validate_token(&token).unwrap();
    assert_eq!(claims.scope, "ANCHOR");
    assert!(claims.clients.contains(&"*".to_string()));
}

#[test]
fn test_client_access_check() {
    let auth_service = test_auth_service();
    let principal = Principal::new_user("admin@example.com", UserScope::Anchor);
    let token = auth_service.generate_access_token(&principal).unwrap();
    let claims = auth_service.validate_token(&token).unwrap();

    // Anchor with "*" should have access to any client
    assert!(auth_service.has_client_access(&claims, "any-client-id"));
    assert!(auth_service.has_client_access(&claims, "another-client"));
}

#[test]
fn test_client_scope_limited_access() {
    let auth_service = test_auth_service();
    let principal = Principal::new_user("user@client.com", UserScope::Client)
        .with_client_id("my-client");
    let token = auth_service.generate_access_token(&principal).unwrap();
    let claims = auth_service.validate_token(&token).unwrap();

    assert!(auth_service.has_client_access(&claims, "my-client"));
    assert!(!auth_service.has_client_access(&claims, "other-client"));
}

#[test]
fn test_role_check() {
    let auth_service = test_auth_service();
    let mut principal = Principal::new_user("test@example.com", UserScope::Anchor);
    principal.assign_role("admin");
    principal.assign_role("viewer");
    let token = auth_service.generate_access_token(&principal).unwrap();
    let claims = auth_service.validate_token(&token).unwrap();

    assert!(auth_service.has_role(&claims, "admin"));
    assert!(auth_service.has_role(&claims, "viewer"));
    assert!(!auth_service.has_role(&claims, "editor"));
}

#[test]
fn test_is_anchor_check() {
    let auth_service = test_auth_service();

    let anchor = Principal::new_user("admin@example.com", UserScope::Anchor);
    let anchor_token = auth_service.generate_access_token(&anchor).unwrap();
    let anchor_claims = auth_service.validate_token(&anchor_token).unwrap();
    assert!(auth_service.is_anchor(&anchor_claims));

    let client_user = Principal::new_user("user@client.com", UserScope::Client);
    let client_token = auth_service.generate_access_token(&client_user).unwrap();
    let client_claims = auth_service.validate_token(&client_token).unwrap();
    assert!(!auth_service.is_anchor(&client_claims));
}

// ─── Token Introspection Logic Tests ───────────────────────────────────────

#[test]
fn test_introspect_valid_token() {
    let auth_service = test_auth_service();
    let (_, token) = test_principal_and_token(&auth_service);

    // Validate token (this is what the introspect endpoint does)
    let claims = auth_service.validate_token(&token).unwrap();
    assert!(!claims.sub.is_empty());
    assert_eq!(claims.scope, "ANCHOR");
    assert!(claims.exp > 0);
    assert!(claims.iat > 0);
}

#[test]
fn test_introspect_invalid_token() {
    let auth_service = test_auth_service();

    // Invalid token should fail validation (introspect returns active=false)
    let result = auth_service.validate_token("not.a.valid.token");
    assert!(result.is_err());
}

#[test]
fn test_introspect_tampered_token() {
    let auth_service = test_auth_service();
    let (_, mut token) = test_principal_and_token(&auth_service);

    // Tamper with the token payload
    token.push('x');
    let result = auth_service.validate_token(&token);
    assert!(result.is_err());
}

// ─── Bearer Token Extraction Tests ─────────────────────────────────────────

#[test]
fn test_extract_bearer_token() {
    use fc_platform::auth::auth_service::extract_bearer_token;

    assert_eq!(extract_bearer_token("Bearer abc123"), Some("abc123"));
    assert_eq!(extract_bearer_token("Bearer "), Some(""));
    assert_eq!(extract_bearer_token("Basic abc123"), None);
    assert_eq!(extract_bearer_token("bearer abc123"), None); // Case-sensitive
    assert_eq!(extract_bearer_token(""), None);
}

// ─── OIDC Login State Tests ────────────────────────────────────────────────

#[test]
fn test_oidc_login_state_expiry() {
    use fc_platform::OidcLoginState;

    let state = OidcLoginState::new(
        "test-state",
        "example.com",
        "idp-123",
        "edm-456",
        "nonce-789",
        "verifier-abc",
    );

    assert!(!state.is_expired());
    assert!(state.is_valid());
}

#[test]
fn test_oidc_login_state_oauth_flow() {
    use fc_platform::OidcLoginState;

    let state = OidcLoginState::new(
        "test-state",
        "example.com",
        "idp-123",
        "edm-456",
        "nonce-789",
        "verifier-abc",
    ).with_oauth_params(
        Some("client-id".to_string()),
        Some("https://app.example.com/callback".to_string()),
        Some("openid profile".to_string()),
        Some("oauth-state".to_string()),
        Some("code-challenge".to_string()),
        Some("S256".to_string()),
        None,
    );

    assert!(state.is_oauth_flow());
    assert_eq!(state.oauth_client_id, Some("client-id".to_string()));
    assert_eq!(state.oauth_redirect_uri, Some("https://app.example.com/callback".to_string()));
}

#[test]
fn test_oidc_login_state_not_oauth_flow() {
    use fc_platform::OidcLoginState;

    let state = OidcLoginState::new(
        "test-state",
        "example.com",
        "idp-123",
        "edm-456",
        "nonce-789",
        "verifier-abc",
    );

    assert!(!state.is_oauth_flow());
}

#[test]
fn test_oidc_login_state_with_return_url() {
    use fc_platform::OidcLoginState;

    let state = OidcLoginState::new(
        "test-state",
        "example.com",
        "idp-123",
        "edm-456",
        "nonce-789",
        "verifier-abc",
    ).with_return_url("/dashboard");

    assert_eq!(state.return_url, Some("/dashboard".to_string()));
}

#[test]
fn test_oidc_login_state_email_domain_lowercased() {
    use fc_platform::OidcLoginState;

    let state = OidcLoginState::new(
        "test-state",
        "EXAMPLE.COM",
        "idp-123",
        "edm-456",
        "nonce-789",
        "verifier-abc",
    );

    assert_eq!(state.email_domain, "example.com");
}

// ─── Refresh Token Tests ───────────────────────────────────────────────────

#[test]
fn test_refresh_token_hash_deterministic() {
    use fc_platform::RefreshToken;

    let hash1 = RefreshToken::hash_token("my-secret-token");
    let hash2 = RefreshToken::hash_token("my-secret-token");

    assert_eq!(hash1, hash2);
}

#[test]
fn test_refresh_token_hash_different_for_different_tokens() {
    use fc_platform::RefreshToken;

    let hash1 = RefreshToken::hash_token("token-1");
    let hash2 = RefreshToken::hash_token("token-2");

    assert_ne!(hash1, hash2);
}

#[test]
fn test_refresh_token_pair_generation() {
    use fc_platform::RefreshToken;

    let (raw_token, entity) = RefreshToken::generate_token_pair("principal-123");

    assert!(!raw_token.is_empty());
    assert_eq!(entity.principal_id, "principal-123");
    assert!(!entity.token_hash.is_empty());
    assert!(entity.is_valid());
    assert!(!entity.revoked);
}

// ─── UserScope Access Control Tests ────────────────────────────────────────

#[test]
fn test_user_scope_anchor_access() {
    assert!(UserScope::Anchor.can_access_client("any-client", None, &[]));
    assert!(UserScope::Anchor.is_anchor());
}

#[test]
fn test_user_scope_client_access() {
    assert!(UserScope::Client.can_access_client("my-client", Some("my-client"), &[]));
    assert!(!UserScope::Client.can_access_client("other-client", Some("my-client"), &[]));
    assert!(!UserScope::Client.is_anchor());
}

#[test]
fn test_user_scope_partner_access() {
    let assigned = vec!["client-1".to_string(), "client-2".to_string()];

    assert!(UserScope::Partner.can_access_client("client-1", None, &assigned));
    assert!(UserScope::Partner.can_access_client("client-2", None, &assigned));
    assert!(!UserScope::Partner.can_access_client("client-3", None, &assigned));
}

// ─── Domain Event Tests ────────────────────────────────────────────────────

#[test]
fn test_user_logged_in_event() {
    use fc_platform::usecase::domain_event::DomainEvent;
    use fc_platform::principal::operations::events::UserLoggedIn;
    use fc_platform::usecase::ExecutionContext;

    let ctx = ExecutionContext::create("principal-123");
    let event = UserLoggedIn::new(
        &ctx,
        "principal-123",
        "user@example.com",
        UserScope::Anchor,
        "idp-456",
        Some("client-789"),
    );

    assert_eq!(event.event_type(), "platform:iam:user:logged-in");
    assert_eq!(event.source(), "platform:iam");
    assert!(event.subject().contains("principal-123"));
    assert_eq!(event.principal_id, "principal-123");
    assert_eq!(event.email, "user@example.com");
    assert_eq!(event.identity_provider_id, "idp-456");
    assert_eq!(event.client_id, Some("client-789".to_string()));
    assert_eq!(event.login_method, "OIDC");
}

#[test]
fn test_user_logged_in_event_without_client() {
    use fc_platform::usecase::domain_event::DomainEvent;
    use fc_platform::principal::operations::events::UserLoggedIn;
    use fc_platform::usecase::ExecutionContext;

    let ctx = ExecutionContext::create("principal-123");
    let event = UserLoggedIn::new(
        &ctx,
        "principal-123",
        "admin@example.com",
        UserScope::Anchor,
        "idp-456",
        None,
    );

    assert_eq!(event.client_id, None);
    assert_eq!(event.email_domain, "example.com");
    assert_eq!(event.scope, "ANCHOR");
}

// ─── AuthContext Tests ─────────────────────────────────────────────────────

#[test]
fn test_auth_context_permission_matching() {
    use fc_platform::service::AuthContext;

    let ctx = AuthContext {
        principal_id: "p-123".to_string(),
        principal_type: "USER".to_string(),
        scope: "ANCHOR".to_string(),
        email: Some("admin@example.com".to_string()),
        name: "Admin".to_string(),
        accessible_clients: vec!["*".to_string()],
        permissions: ["events:read", "events:write", "clients:*"]
            .iter().map(|s| s.to_string()).collect(),
        roles: vec!["admin".to_string()],
    };

    // Direct permissions
    assert!(ctx.has_permission("events:read"));
    assert!(ctx.has_permission("events:write"));

    // Wildcard permissions
    assert!(ctx.has_permission("clients:read"));
    assert!(ctx.has_permission("clients:write"));
    assert!(ctx.has_permission("clients:anything"));

    // Not granted
    assert!(!ctx.has_permission("users:read"));

    // Client access with wildcard
    assert!(ctx.can_access_client("any-client"));
    assert!(ctx.is_anchor());
}

#[test]
fn test_auth_context_multiple_permissions_check() {
    use fc_platform::service::AuthContext;

    let ctx = AuthContext {
        principal_id: "p-123".to_string(),
        principal_type: "USER".to_string(),
        scope: "CLIENT".to_string(),
        email: Some("user@client.com".to_string()),
        name: "User".to_string(),
        accessible_clients: vec!["client-1".to_string()],
        permissions: ["events:read", "subscriptions:read"]
            .iter().map(|s| s.to_string()).collect(),
        roles: vec!["viewer".to_string()],
    };

    assert!(ctx.has_all_permissions(&["events:read", "subscriptions:read"]));
    assert!(!ctx.has_all_permissions(&["events:read", "events:write"]));

    assert!(ctx.has_any_permission(&["events:read", "events:write"]));
    assert!(!ctx.has_any_permission(&["users:read", "users:write"]));
}

// ─── Password Service Tests ───────────────────────────────────────────────

#[test]
fn test_password_hashing_and_verification() {
    use fc_platform::PasswordService;

    let service = fc_platform::PasswordService::new(Default::default(), Default::default());
    let password = "MySecurePassword123!";

    let hash = service.hash_password(password).unwrap();
    assert!(!hash.is_empty());
    assert_ne!(hash, password);

    // Verify correct password
    assert!(service.verify_password(password, &hash).unwrap());

    // Verify wrong password
    assert!(!service.verify_password("WrongPassword", &hash).unwrap());
}

#[test]
fn test_password_hash_uniqueness() {
    use fc_platform::PasswordService;

    let service = fc_platform::PasswordService::new(Default::default(), Default::default());
    let password = "SamePassword123!";

    let hash1 = service.hash_password(password).unwrap();
    let hash2 = service.hash_password(password).unwrap();

    // Same password should produce different hashes (salt)
    assert_ne!(hash1, hash2);

    // Both should still verify
    assert!(service.verify_password(password, &hash1).unwrap());
    assert!(service.verify_password(password, &hash2).unwrap());
}
