//! The verifier over a fake platform's real RS256 tokens.

use std::sync::Arc;

use fc_platform_jwks::testing::{Claims, TestPlatform, AUTHORIZATION_ENDPOINT, ISSUER};
use fc_platform_jwks::{clock, BearerAuthenticator, JwksKeySource};

fn authenticator(platform_url: &str) -> BearerAuthenticator {
    let keys = Arc::new(JwksKeySource::new(
        reqwest::Client::new(),
        platform_url,
        clock::system(),
    ));
    BearerAuthenticator::new(keys, clock::system())
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

#[tokio::test]
async fn an_api_token_verifies_with_its_scope_and_token_use() {
    let platform = TestPlatform::start().await;
    let auth = authenticator(&platform.url);
    let token = platform.mint(&Claims::api(&[
        "platform:messaging:router:view",
        "hr:staff:record:view",
    ]));
    let claims = auth.authenticate(Some(&bearer(&token))).await.unwrap();
    assert_eq!(claims.subject, "prn_test");
    assert!(claims.is_api_token());
    assert!(claims.grants("platform:messaging:router:view"));
    assert!(!claims.grants("platform:messaging:router:operate"));
    assert_eq!(auth.key_source().issuer().as_deref(), Some(ISSUER));
}

#[tokio::test]
async fn a_session_token_verifies_but_is_not_an_api_token() {
    let platform = TestPlatform::start().await;
    let auth = authenticator(&platform.url);
    let token = platform.mint(&Claims::api(&[]).token_use(None));
    let claims = auth.authenticate(Some(&bearer(&token))).await.unwrap();
    assert!(!claims.is_api_token());
}

#[tokio::test]
async fn refused_tokens() {
    let platform = TestPlatform::start().await;
    let auth = authenticator(&platform.url);
    let view = ["platform:messaging:router:view"];

    let identity = platform.mint(&Claims::api(&view).token_use(Some("identity")));
    assert_eq!(
        auth.authenticate(Some(&bearer(&identity)))
            .await
            .unwrap_err(),
        "an identity token is not an API credential"
    );

    let expired = platform.mint(&Claims::api(&view).expires_in(-10));
    assert!(auth
        .authenticate(Some(&bearer(&expired)))
        .await
        .unwrap_err()
        .contains("expired"));

    let other_issuer = platform.mint_with_issuer(&platform.url, &Claims::api(&view));
    assert!(auth
        .authenticate(Some(&bearer(&other_issuer)))
        .await
        .unwrap_err()
        .contains("issuer"));

    let forged = platform.mint_forged(&Claims::api(&view));
    assert_eq!(
        auth.authenticate(Some(&bearer(&forged))).await.unwrap_err(),
        "token signature is invalid"
    );

    let foreign_audience = platform.mint(&Claims::api(&view).audience("some-client"));
    assert!(auth
        .authenticate(Some(&bearer(&foreign_audience)))
        .await
        .unwrap_err()
        .contains("audience"));

    assert_eq!(
        auth.authenticate(None).await.unwrap_err(),
        "missing bearer token"
    );
    assert_eq!(
        auth.authenticate(Some("Basic abc")).await.unwrap_err(),
        "missing bearer token"
    );
}

#[tokio::test]
async fn the_keys_are_cached() {
    let platform = TestPlatform::start().await;
    let auth = authenticator(&platform.url);
    let token = platform.mint(&Claims::api(&[]));
    for _ in 0..5 {
        auth.authenticate(Some(&bearer(&token))).await.unwrap();
    }
    assert_eq!(platform.jwks_requests(), 1);
}

#[tokio::test]
async fn discovery_names_the_authorization_endpoint() {
    let platform = TestPlatform::start().await;
    let keys = JwksKeySource::new(
        reqwest::Client::new(),
        platform.url.clone(),
        clock::system(),
    );
    assert_eq!(
        keys.authorization_endpoint().await.as_deref(),
        Some(AUTHORIZATION_ENDPOINT)
    );
}

#[tokio::test]
async fn no_platform_refuses_every_token_and_discovery_backs_off() {
    // Nothing listens on port 9 (discard) locally.
    let keys = Arc::new(JwksKeySource::new(
        reqwest::Client::new(),
        "http://127.0.0.1:9",
        clock::system(),
    ));
    assert!(!keys.ensure_discovered().await);
    // Within the 30 s floor, no second attempt.
    assert!(!keys.ensure_discovered().await);
    assert_eq!(keys.authorization_endpoint().await, None);
    let auth = BearerAuthenticator::new(keys, clock::system());
    let platform = TestPlatform::start().await;
    let token = platform.mint(&Claims::api(&[]));
    assert!(auth.authenticate(Some(&bearer(&token))).await.is_err());
}
