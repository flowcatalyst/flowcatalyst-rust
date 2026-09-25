package io.flowcatalyst.sdk.error;

import com.fasterxml.jackson.databind.JsonNode;
import java.time.Duration;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

/**
 * Typed SDK errors, mirroring the TypeScript SDK's discriminated union.
 * Every failure surfaced by the SDK is one of these variants, carried by a
 * {@link FlowCatalystException}. Handle exhaustively with a pattern-matching
 * {@code switch}:
 *
 * <pre>{@code
 * try {
 *     client.eventTypes().list(null);
 * } catch (FlowCatalystException e) {
 *     switch (e.error()) {
 *         case SdkError.NotFound nf -> ...;
 *         case SdkError.RateLimited rl -> ...;
 *         default -> throw e;
 *     }
 * }
 * }</pre>
 */
public sealed interface SdkError {

    /** Human-readable description of the failure. */
    String message();

    // ── Authentication ──────────────────────────────────────────────

    /** Client ID / secret not configured. */
    record MissingCredentials(String message) implements SdkError {}

    /** The OAuth server rejected the client credentials (401/403 on token fetch). */
    record InvalidCredentials(String message) implements SdkError {}

    /** A request was rejected with 401 (and could not be transparently refreshed). */
    record TokenExpired(String message) implements SdkError {}

    /** The token endpoint call itself failed. */
    record TokenFetchFailed(String message, Throwable cause) implements SdkError {}

    // ── HTTP / network ──────────────────────────────────────────────

    /** Connection-level failure (DNS, refused, reset, malformed response). */
    record Network(String message, Throwable cause) implements SdkError {}

    /** The request exceeded the configured timeout. */
    record Timeout(String message, Duration duration) implements SdkError {}

    /** Any non-2xx status without a more specific variant. */
    record HttpStatus(int status, String message, String body) implements SdkError {}

    // ── Mapped API errors ───────────────────────────────────────────

    /** 422 — request body failed validation; {@code errors} maps field → messages. */
    record Validation(String message, Map<String, List<String>> errors) implements SdkError {}

    /** 404. */
    record NotFound(String message) implements SdkError {}

    /** 403. */
    record Forbidden(String message) implements SdkError {}

    /** 409 — {@code code} is the platform error code when present. */
    record Conflict(String message, String code) implements SdkError {}

    /** 429 — {@code retryAfter} is populated from the response when present. */
    record RateLimited(String message, Duration retryAfter) implements SdkError {}

    // ── Client-side, multi-part operations ──────────────────────────

    /**
     * One or more parts of a client-side, multi-step operation failed —
     * distinct from every variant above, each of which describes ONE failed
     * HTTP call. {@code message} names every failure. Used by {@code
     * io.flowcatalyst.sdk.sync.DefinitionSynchronizer} (via {@code
     * DefinitionSyncException}, a {@link io.flowcatalyst.sdk.error.FlowCatalystException}
     * subclass) when syncing several categories/scopes and some — but not
     * necessarily all — of them failed; the exception's typed accessors
     * carry whatever DID complete.
     */
    record PartialFailure(String message) implements SdkError {}

    /**
     * Map an HTTP status + parsed JSON error body to the matching variant.
     * Platform error JSON is {@code {"error": "<CODE>", "message": "<text>"}};
     * the human message is preferred, falling back to the code.
     */
    static SdkError fromHttpStatus(int status, JsonNode body, String rawBody) {
        String message = null;
        String code = null;
        if (body != null && body.isObject()) {
            if (body.hasNonNull("message")) message = body.get("message").asText();
            if (body.hasNonNull("error")) {
                code = body.get("error").asText();
                if (message == null) message = code;
            }
        }
        if (message == null) message = "HTTP " + status;

        return switch (status) {
            case 401 -> new TokenExpired(message);
            case 403 -> new Forbidden(message);
            case 404 -> new NotFound(message);
            case 409 -> new Conflict(message, code);
            case 422 -> new Validation(message, validationErrors(body));
            case 429 -> new RateLimited(message, retryAfter(body));
            default -> new HttpStatus(status, message, rawBody);
        };
    }

    private static Map<String, List<String>> validationErrors(JsonNode body) {
        Map<String, List<String>> errors = new HashMap<>();
        if (body != null && body.has("errors") && body.get("errors").isObject()) {
            body.get("errors").properties().forEach(entry -> {
                List<String> messages = new ArrayList<>();
                if (entry.getValue().isArray()) {
                    entry.getValue().forEach(m -> messages.add(m.asText()));
                } else {
                    messages.add(entry.getValue().asText());
                }
                errors.put(entry.getKey(), messages);
            });
        }
        return errors;
    }

    private static Duration retryAfter(JsonNode body) {
        if (body != null && body.hasNonNull("retryAfter") && body.get("retryAfter").isNumber()) {
            return Duration.ofSeconds(body.get("retryAfter").asLong());
        }
        return null;
    }
}
