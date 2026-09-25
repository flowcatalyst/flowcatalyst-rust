package io.flowcatalyst.sdk.outbox;

import java.time.Instant;
import java.util.LinkedHashMap;
import java.util.Map;

/**
 * DTO for creating a dispatch job in the outbox. Immutable builder — all
 * {@code with*} methods return a new instance.
 *
 * <pre>{@code
 * var job = CreateDispatchJobDto
 *         .create("order-service", "orders:sales:order:process",
 *                 "https://api.example.com/webhook", "{\"orderId\":\"123\"}", "pool-1")
 *         .withCorrelationId("corr-789")
 *         .withTimeoutSeconds(60);
 * }</pre>
 */
public final class CreateDispatchJobDto {

    /**
     * Ordering behavior within a message group.
     * IMMEDIATE: no ordering, jobs dispatch concurrently (platform default).
     * NEXT_ON_ERROR: FIFO per group; a failed job is retried later but the group moves on.
     * BLOCK_ON_ERROR: strict FIFO per group; a failed job blocks the group until resolved.
     */
    public enum DispatchMode {
        IMMEDIATE,
        NEXT_ON_ERROR,
        BLOCK_ON_ERROR
    }

    private String source;
    private String code;
    private String targetUrl;
    private String payload;
    private String dispatchPoolId;
    private String subject;
    private String correlationId;
    private String eventId;
    private Map<String, String> metadata = Map.of();
    private Map<String, String> headers = Map.of();
    private String payloadContentType = "application/json";
    private boolean dataOnly = true;
    private String messageGroup;
    private DispatchMode mode;
    private Integer sequence;
    private int timeoutSeconds = 30;
    private int maxRetries = 5;
    private String retryStrategy;
    private Instant scheduledFor;
    private Instant expiresAt;
    private String idempotencyKey;
    private String externalId;
    private String connectionId;

    private CreateDispatchJobDto() {}

    private CreateDispatchJobDto copy() {
        CreateDispatchJobDto c = new CreateDispatchJobDto();
        c.source = source;
        c.code = code;
        c.targetUrl = targetUrl;
        c.payload = payload;
        c.dispatchPoolId = dispatchPoolId;
        c.subject = subject;
        c.correlationId = correlationId;
        c.eventId = eventId;
        c.metadata = metadata;
        c.headers = headers;
        c.payloadContentType = payloadContentType;
        c.dataOnly = dataOnly;
        c.messageGroup = messageGroup;
        c.mode = mode;
        c.sequence = sequence;
        c.timeoutSeconds = timeoutSeconds;
        c.maxRetries = maxRetries;
        c.retryStrategy = retryStrategy;
        c.scheduledFor = scheduledFor;
        c.expiresAt = expiresAt;
        c.idempotencyKey = idempotencyKey;
        c.externalId = externalId;
        c.connectionId = connectionId;
        return c;
    }

    /**
     * @param code fully qualified {@code application:subdomain:aggregate:action}
     *             code — the platform facets on its segments and resolves
     *             delivery signing credentials from the application segment;
     *             bare codes are rejected
     * @param payload the payload string (serialize objects to JSON first)
     */
    public static CreateDispatchJobDto create(
            String source, String code, String targetUrl, String payload, String dispatchPoolId) {
        QualifiedCode.assertQualified(code, "Dispatch job code");
        CreateDispatchJobDto dto = new CreateDispatchJobDto();
        dto.source = source;
        dto.code = code;
        dto.targetUrl = targetUrl;
        dto.payload = payload;
        dto.dispatchPoolId = dispatchPoolId;
        return dto;
    }

    public CreateDispatchJobDto withSubject(String subject) {
        CreateDispatchJobDto c = copy();
        c.subject = subject;
        return c;
    }

    public CreateDispatchJobDto withCorrelationId(String correlationId) {
        CreateDispatchJobDto c = copy();
        c.correlationId = correlationId;
        return c;
    }

    public CreateDispatchJobDto withEventId(String eventId) {
        CreateDispatchJobDto c = copy();
        c.eventId = eventId;
        return c;
    }

    /** Merge additional metadata (existing keys are overwritten). */
    public CreateDispatchJobDto withMetadata(Map<String, String> extra) {
        CreateDispatchJobDto c = copy();
        Map<String, String> merged = new LinkedHashMap<>(metadata);
        merged.putAll(extra);
        c.metadata = Map.copyOf(merged);
        return c;
    }

    /** Merge additional headers (existing keys are overwritten). */
    public CreateDispatchJobDto withHeaders(Map<String, String> extra) {
        CreateDispatchJobDto c = copy();
        Map<String, String> merged = new LinkedHashMap<>(headers);
        merged.putAll(extra);
        c.headers = Map.copyOf(merged);
        return c;
    }

    public CreateDispatchJobDto withPayloadContentType(String payloadContentType) {
        CreateDispatchJobDto c = copy();
        c.payloadContentType = payloadContentType;
        return c;
    }

    public CreateDispatchJobDto withDataOnly(boolean dataOnly) {
        CreateDispatchJobDto c = copy();
        c.dataOnly = dataOnly;
        return c;
    }

    public CreateDispatchJobDto withMessageGroup(String messageGroup) {
        CreateDispatchJobDto c = copy();
        c.messageGroup = messageGroup;
        return c;
    }

    /** Ordering behavior within the message group; unset defaults to IMMEDIATE on the platform. */
    public CreateDispatchJobDto withMode(DispatchMode mode) {
        CreateDispatchJobDto c = copy();
        c.mode = mode;
        return c;
    }

    public CreateDispatchJobDto withSequence(int sequence) {
        CreateDispatchJobDto c = copy();
        c.sequence = sequence;
        return c;
    }

    public CreateDispatchJobDto withTimeoutSeconds(int timeoutSeconds) {
        CreateDispatchJobDto c = copy();
        c.timeoutSeconds = timeoutSeconds;
        return c;
    }

    public CreateDispatchJobDto withMaxRetries(int maxRetries) {
        CreateDispatchJobDto c = copy();
        c.maxRetries = maxRetries;
        return c;
    }

    public CreateDispatchJobDto withRetryStrategy(String retryStrategy) {
        CreateDispatchJobDto c = copy();
        c.retryStrategy = retryStrategy;
        return c;
    }

    public CreateDispatchJobDto withScheduledFor(Instant scheduledFor) {
        CreateDispatchJobDto c = copy();
        c.scheduledFor = scheduledFor;
        return c;
    }

    public CreateDispatchJobDto withExpiresAt(Instant expiresAt) {
        CreateDispatchJobDto c = copy();
        c.expiresAt = expiresAt;
        return c;
    }

    public CreateDispatchJobDto withIdempotencyKey(String idempotencyKey) {
        CreateDispatchJobDto c = copy();
        c.idempotencyKey = idempotencyKey;
        return c;
    }

    public CreateDispatchJobDto withExternalId(String externalId) {
        CreateDispatchJobDto c = copy();
        c.externalId = externalId;
        return c;
    }

    public CreateDispatchJobDto withConnectionId(String connectionId) {
        CreateDispatchJobDto c = copy();
        c.connectionId = connectionId;
        return c;
    }

    public String messageGroup() {
        return messageGroup;
    }

    /** Build the dispatch job payload for the outbox (nulls omitted). */
    Map<String, Object> toPayload() {
        Map<String, Object> payload = new LinkedHashMap<>();
        payload.put("source", source);
        payload.put("code", code);
        payload.put("targetUrl", targetUrl);
        payload.put("payload", this.payload);
        payload.put("payloadContentType", payloadContentType);
        payload.put("dispatchPoolId", dispatchPoolId);
        putIfNotNull(payload, "subject", subject);
        putIfNotNull(payload, "correlationId", correlationId);
        putIfNotNull(payload, "eventId", eventId);
        if (!metadata.isEmpty()) payload.put("metadata", metadata);
        if (!headers.isEmpty()) payload.put("headers", headers);
        payload.put("dataOnly", dataOnly);
        putIfNotNull(payload, "messageGroup", messageGroup);
        putIfNotNull(payload, "mode", mode != null ? mode.name() : null);
        putIfNotNull(payload, "sequence", sequence);
        payload.put("timeoutSeconds", timeoutSeconds);
        payload.put("maxRetries", maxRetries);
        putIfNotNull(payload, "retryStrategy", retryStrategy);
        putIfNotNull(payload, "scheduledFor", scheduledFor != null ? scheduledFor.toString() : null);
        putIfNotNull(payload, "expiresAt", expiresAt != null ? expiresAt.toString() : null);
        putIfNotNull(payload, "idempotencyKey", idempotencyKey);
        putIfNotNull(payload, "externalId", externalId);
        putIfNotNull(payload, "connectionId", connectionId);
        return payload;
    }

    private static void putIfNotNull(Map<String, Object> map, String key, Object value) {
        if (value != null) map.put(key, value);
    }
}
