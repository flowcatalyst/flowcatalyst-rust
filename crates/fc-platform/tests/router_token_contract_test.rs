//! The tokens the platform mints, through the verifier the router's API
//! uses (`fc-platform-jwks`, owner ruling 2): the platform's own discovery
//! document and JWKS, served as the router reaches them over an internal
//! address, with the external issuer in the tokens.
//!
//! - a client-credentials token (`token_use=api`, `scope` = the granted
//!   permissions) passes and grants `router:view`;
//! - a token without a scope passes but grants nothing (the router answers
//!   403);
//! - an identity token is refused; a session cookie's token is not an API
//!   token (the router answers 401).

use std::sync::Arc;

use fc_platform::auth::auth_service::{AuthConfig, AuthService};
use fc_platform::shared::well_known_api::{well_known_router, WellKnownState};
use fc_platform::{Principal, UserScope};
use fc_platform_jwks::{clock, BearerAuthenticator, JwksKeySource};

const ISSUER: &str = "https://platform.example.test";
const VIEW: &str = "platform:messaging:router:view";
const OPERATE: &str = "platform:messaging:router:operate";

#[tokio::test]
async fn platform_tokens_pass_the_router_verifier() {
    let (private_pem, public_pem) = AuthConfig::generate_rsa_keys(None).unwrap();
    let auth = Arc::new(AuthService::new(AuthConfig {
        rsa_private_key: Some(private_pem),
        rsa_public_key: Some(public_pem),
        issuer: ISSUER.to_string(),
        audience: ISSUER.to_string(),
        ..AuthConfig::default()
    }));
    let app = axum::Router::new().nest(
        "/.well-known",
        well_known_router(WellKnownState {
            auth_service: auth.clone(),
            external_base_url: ISSUER.to_string(),
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let internal = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let keys = Arc::new(JwksKeySource::new(
        reqwest::Client::new(),
        internal,
        clock::system(),
    ));
    let verifier = BearerAuthenticator::new(keys.clone(), clock::system());
    let bearer = |t: &str| format!("Bearer {t}");

    let mut app_sa = Principal::new_service("app-svc", "App service", UserScope::Anchor);
    app_sa.assign_role("platform:application-service");

    // client_credentials: the scope carries the granted permissions.
    let token = auth
        .generate_access_token_with_scope(&app_sa, &[VIEW.to_string()], None)
        .unwrap();
    let claims = verifier.authenticate(Some(&bearer(&token))).await.unwrap();
    assert!(claims.is_api_token());
    assert!(claims.grants(VIEW));
    assert!(!claims.grants(OPERATE));
    assert_eq!(keys.issuer().as_deref(), Some(ISSUER));
    assert_eq!(
        keys.authorization_endpoint().await.as_deref(),
        Some("https://platform.example.test/oauth/authorize")
    );

    // A super-admin's granted scope holds the wildcard.
    let wildcard = auth
        .generate_access_token_with_scope(&app_sa, &["platform:*:*:*".to_string()], None)
        .unwrap();
    let claims = verifier
        .authenticate(Some(&bearer(&wildcard)))
        .await
        .unwrap();
    assert!(claims.grants(VIEW) && claims.grants(OPERATE));

    // No scope: verified, but nothing granted.
    let unscoped = auth.generate_access_token(&app_sa).unwrap();
    let claims = verifier
        .authenticate(Some(&bearer(&unscoped)))
        .await
        .unwrap();
    assert!(claims.is_api_token());
    assert!(!claims.grants(VIEW));

    // An identity token is refused outright.
    let identity = auth.generate_identity_access_token(&app_sa, None).unwrap();
    assert_eq!(
        verifier
            .authenticate(Some(&bearer(&identity)))
            .await
            .unwrap_err(),
        "an identity token is not an API credential"
    );

    // A session cookie's token carries no token_use: not an API token.
    let session = auth.generate_session_token(&app_sa).unwrap();
    if let Ok(claims) = verifier.authenticate(Some(&bearer(&session))).await {
        assert!(!claims.is_api_token());
    }
}
