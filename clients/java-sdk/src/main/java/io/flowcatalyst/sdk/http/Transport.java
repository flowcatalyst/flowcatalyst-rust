package io.flowcatalyst.sdk.http;

import com.fasterxml.jackson.databind.JavaType;
import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import io.flowcatalyst.sdk.auth.ClientCredentialsTokenManager;
import io.flowcatalyst.sdk.auth.TokenProvider;
import io.flowcatalyst.sdk.error.FlowCatalystException;
import io.flowcatalyst.sdk.error.SdkError;
import java.io.IOException;
import java.net.URI;
import java.net.URLEncoder;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.net.http.HttpTimeoutException;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.Collection;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Set;

/**
 * Authenticated HTTP execution shared by all resources: bearer injection,
 * exponential-backoff retry on transient statuses (408/429/502/503/504),
 * a one-shot token refresh on 401 (client-credentials mode only), and typed
 * error mapping. Blocking by design — run on virtual threads to scale.
 */
public final class Transport {

    private static final Set<Integer> RETRYABLE = Set.of(408, 429, 502, 503, 504);

    private final HttpClient http;
    private final ObjectMapper mapper = Json.newMapper();
    private final String baseUrl;
    private final TokenProvider tokenProvider;
    private final ClientCredentialsTokenManager tokenManager; // null in user-token mode
    private final Duration timeout;
    private final int retryAttempts;
    private final Duration retryDelay;

    public Transport(
            String baseUrl,
            TokenProvider tokenProvider,
            ClientCredentialsTokenManager tokenManager,
            Duration timeout,
            int retryAttempts,
            Duration retryDelay) {
        this.baseUrl = baseUrl;
        this.tokenProvider = tokenProvider;
        this.tokenManager = tokenManager;
        this.timeout = timeout;
        this.retryAttempts = retryAttempts;
        this.retryDelay = retryDelay;
        this.http = HttpClient.newBuilder().connectTimeout(timeout).build();
    }

    public ObjectMapper mapper() {
        return mapper;
    }

    // ── Convenience methods used by resources ───────────────────────

    public <T> T get(String path, Map<String, Object> query, Class<T> type) {
        return execute("GET", baseUrl + path, query, null, javaType(type), true);
    }

    public <T> T get(String path, Map<String, Object> query, JavaType type) {
        return execute("GET", baseUrl + path, query, null, type, true);
    }

    public <T> T post(String path, Object body, Class<T> type) {
        return execute("POST", baseUrl + path, null, body, javaType(type), true);
    }

    public <T> T post(String path, Map<String, Object> query, Object body, Class<T> type) {
        return execute("POST", baseUrl + path, query, body, javaType(type), true);
    }

    public <T> T put(String path, Object body, Class<T> type) {
        return execute("PUT", baseUrl + path, null, body, javaType(type), true);
    }

    public <T> T delete(String path, Class<T> type) {
        return execute("DELETE", baseUrl + path, null, null, javaType(type), true);
    }

    /** Unauthenticated request against an absolute URL (router monitoring). */
    public <T> T rawUnauthenticated(
            String method, String url, Map<String, Object> query, Object body, JavaType type) {
        return execute(method, url, query, body, type, false);
    }

    /** URL-encode a path segment. */
    public static String enc(String segment) {
        return URLEncoder.encode(segment, StandardCharsets.UTF_8).replace("+", "%20");
    }

    private JavaType javaType(Class<?> type) {
        return type == null ? null : mapper.getTypeFactory().constructType(type);
    }

    public JavaType listOf(Class<?> element) {
        return mapper.getTypeFactory().constructCollectionType(java.util.List.class, element);
    }

    public JavaType mapOf(Class<?> key, Class<?> value) {
        return mapper.getTypeFactory().constructMapType(java.util.Map.class, key, value);
    }

    // ── Core execution ──────────────────────────────────────────────

    @SuppressWarnings("unchecked")
    private <T> T execute(
            String method,
            String url,
            Map<String, Object> query,
            Object body,
            JavaType responseType,
            boolean authenticated) {
        String fullUrl = url + queryString(query);
        String token = null;
        boolean canRefreshToken = authenticated && tokenManager != null;

        if (authenticated) {
            token = acquireToken();
        }

        int attempt = 0;
        while (true) {
            HttpResponse<String> response = send(method, fullUrl, body, token);
            int status = response.statusCode();

            if (status >= 200 && status < 300) {
                return (T) parseResponse(response.body(), status, responseType);
            }

            // One-shot transparent refresh on 401 (client-credentials mode only).
            if (status == 401 && attempt == 0 && canRefreshToken) {
                token = tokenManager.refreshToken();
                attempt++;
                continue;
            }

            if (RETRYABLE.contains(status) && attempt < retryAttempts) {
                sleep(retryDelay.multipliedBy(1L << attempt));
                attempt++;
                continue;
            }

            throw new FlowCatalystException(
                    SdkError.fromHttpStatus(status, parseQuietly(response.body()), response.body()));
        }
    }

    private String acquireToken() {
        try {
            return tokenProvider.getToken();
        } catch (FlowCatalystException e) {
            throw e;
        } catch (RuntimeException e) {
            throw new FlowCatalystException(new SdkError.TokenExpired(
                    e.getMessage() != null ? e.getMessage() : "Failed to get access token"));
        }
    }

    private HttpResponse<String> send(String method, String url, Object body, String token) {
        HttpRequest.Builder builder = HttpRequest.newBuilder(URI.create(url))
                .timeout(timeout)
                .header("Accept", "application/json");
        if (token != null) {
            builder.header("Authorization", "Bearer " + token);
        }
        if (body != null) {
            builder.header("Content-Type", "application/json");
            builder.method(method, HttpRequest.BodyPublishers.ofString(serialize(body)));
        } else {
            builder.method(method, HttpRequest.BodyPublishers.noBody());
        }

        try {
            return http.send(builder.build(), HttpResponse.BodyHandlers.ofString());
        } catch (HttpTimeoutException e) {
            throw new FlowCatalystException(new SdkError.Timeout(
                    "Request timed out after " + timeout.toMillis() + "ms", timeout));
        } catch (IOException e) {
            throw new FlowCatalystException(new SdkError.Network(
                    e.getMessage() != null ? e.getMessage() : "Network error", e));
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            throw new FlowCatalystException(new SdkError.Network("Request interrupted", e));
        }
    }

    private String serialize(Object body) {
        try {
            return mapper.writeValueAsString(body);
        } catch (IOException e) {
            throw new FlowCatalystException(
                    new SdkError.Network("Failed to serialize request body: " + e.getMessage(), e));
        }
    }

    private Object parseResponse(String body, int status, JavaType responseType) {
        if (responseType == null || status == 204 || body == null || body.isEmpty()) {
            return null;
        }
        try {
            return mapper.readValue(body, responseType);
        } catch (IOException e) {
            throw new FlowCatalystException(
                    new SdkError.Network("Failed to parse response: " + e.getMessage(), e));
        }
    }

    private JsonNode parseQuietly(String body) {
        try {
            return body == null || body.isEmpty() ? null : mapper.readTree(body);
        } catch (IOException e) {
            return null;
        }
    }

    private static String queryString(Map<String, Object> query) {
        if (query == null || query.isEmpty()) return "";
        StringBuilder sb = new StringBuilder();
        Map<String, Object> ordered = new LinkedHashMap<>(query);
        ordered.forEach((key, value) -> {
            if (value == null) return;
            if (value instanceof Collection<?> values) {
                for (Object v : values) appendParam(sb, key, v);
            } else {
                appendParam(sb, key, value);
            }
        });
        return sb.isEmpty() ? "" : "?" + sb;
    }

    private static void appendParam(StringBuilder sb, String key, Object value) {
        if (!sb.isEmpty()) sb.append('&');
        sb.append(URLEncoder.encode(key, StandardCharsets.UTF_8))
                .append('=')
                .append(URLEncoder.encode(String.valueOf(value), StandardCharsets.UTF_8));
    }

    private static void sleep(Duration duration) {
        try {
            Thread.sleep(duration.toMillis());
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            throw new FlowCatalystException(new SdkError.Network("Retry interrupted", e));
        }
    }
}
