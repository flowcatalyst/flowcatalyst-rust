package io.flowcatalyst.sdk.auth;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import io.flowcatalyst.sdk.error.FlowCatalystException;
import io.flowcatalyst.sdk.error.SdkError;
import java.io.IOException;
import java.net.URI;
import java.net.URLEncoder;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.time.Instant;

/**
 * OAuth2 client-credentials token manager with caching. Tokens are cached
 * in memory and refreshed when within 60 seconds of expiry. Fetches are
 * single-flight: concurrent callers block on the in-progress fetch instead
 * of issuing their own.
 */
public final class ClientCredentialsTokenManager implements TokenProvider {

    private static final Duration EXPIRY_BUFFER = Duration.ofSeconds(60);

    private final String tokenUrl;
    private final String clientId;
    private final String clientSecret;
    private final HttpClient http;
    private final ObjectMapper mapper = new ObjectMapper();

    private String cachedToken;
    private Instant expiresAt;

    /**
     * @param baseUrl  platform base URL; the token endpoint defaults to
     *                 {@code {baseUrl}/oauth/token}
     * @param tokenUrl optional custom token endpoint (nullable)
     */
    public ClientCredentialsTokenManager(
            String baseUrl, String clientId, String clientSecret, String tokenUrl) {
        String trimmed = baseUrl == null ? "" : baseUrl.replaceAll("/$", "");
        this.tokenUrl = tokenUrl != null ? tokenUrl : trimmed + "/oauth/token";
        this.clientId = clientId;
        this.clientSecret = clientSecret;
        this.http = HttpClient.newBuilder().connectTimeout(Duration.ofSeconds(10)).build();
    }

    @Override
    public synchronized String getToken() {
        if (cachedToken != null && expiresAt != null
                && Instant.now().plus(EXPIRY_BUFFER).isBefore(expiresAt)) {
            return cachedToken;
        }
        return fetchNewToken();
    }

    /** Discard the cached token and fetch a fresh one. */
    public synchronized String refreshToken() {
        cachedToken = null;
        expiresAt = null;
        return fetchNewToken();
    }

    public boolean hasCredentials() {
        return clientId != null && !clientId.isEmpty()
                && clientSecret != null && !clientSecret.isEmpty();
    }

    /** Clear the cached token without fetching a new one. */
    public synchronized void clearCache() {
        cachedToken = null;
        expiresAt = null;
    }

    private String fetchNewToken() {
        if (!hasCredentials()) {
            throw new FlowCatalystException(
                    new SdkError.MissingCredentials("Client ID and secret are required"));
        }

        String form = "grant_type=client_credentials"
                + "&client_id=" + URLEncoder.encode(clientId, StandardCharsets.UTF_8)
                + "&client_secret=" + URLEncoder.encode(clientSecret, StandardCharsets.UTF_8);
        HttpRequest request = HttpRequest.newBuilder(URI.create(tokenUrl))
                .header("Content-Type", "application/x-www-form-urlencoded")
                .header("Accept", "application/json")
                .POST(HttpRequest.BodyPublishers.ofString(form))
                .build();

        HttpResponse<String> response;
        try {
            response = http.send(request, HttpResponse.BodyHandlers.ofString());
        } catch (IOException e) {
            throw new FlowCatalystException(
                    new SdkError.TokenFetchFailed(e.getMessage(), e));
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            throw new FlowCatalystException(
                    new SdkError.TokenFetchFailed("Token fetch interrupted", e));
        }

        int status = response.statusCode();
        if (status == 401 || status == 403) {
            throw new FlowCatalystException(
                    new SdkError.InvalidCredentials("Invalid client credentials"));
        }

        JsonNode body = parseQuietly(response.body());
        if (status < 200 || status >= 300) {
            String message = "Token fetch failed";
            if (body != null) {
                if (body.hasNonNull("error_description")) {
                    message = body.get("error_description").asText();
                } else if (body.hasNonNull("error")) {
                    message = body.get("error").asText();
                }
            }
            throw new FlowCatalystException(new SdkError.TokenFetchFailed(message, null));
        }

        if (body == null || !body.hasNonNull("access_token")) {
            throw new FlowCatalystException(
                    new SdkError.TokenFetchFailed("No access token in response", null));
        }

        cachedToken = body.get("access_token").asText();
        long expiresIn = body.hasNonNull("expires_in") ? body.get("expires_in").asLong() : 0;
        expiresAt = Instant.now().plusSeconds(expiresIn);
        return cachedToken;
    }

    private JsonNode parseQuietly(String body) {
        try {
            return mapper.readTree(body);
        } catch (IOException e) {
            return null;
        }
    }
}
