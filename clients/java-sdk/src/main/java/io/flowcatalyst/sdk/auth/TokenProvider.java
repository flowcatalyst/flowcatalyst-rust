package io.flowcatalyst.sdk.auth;

/**
 * Supplies the bearer token for API requests. Implementations may be a static
 * token, a delegating supplier owned by the host application, or the SDK's
 * {@link ClientCredentialsTokenManager}.
 */
@FunctionalInterface
public interface TokenProvider {

    /** Return a currently-valid access token. */
    String getToken();
}
