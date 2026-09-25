package io.flowcatalyst.sdk.outbox;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * DTO for creating an event in the outbox. Immutable builder — all
 * {@code with*} methods return a new instance.
 *
 * <pre>{@code
 * var event = CreateEventDto
 *         .create("orders:sales:order:created", Map.of("orderId", "123"))
 *         .withSource("order-service")
 *         .withCorrelationId("corr-456");
 * }</pre>
 */
public final class CreateEventDto {

    /** A context-data key/value pair. */
    public record ContextEntry(String key, String value) {}

    private final String type;
    private final Map<String, Object> data;
    private final String source;
    private final String subject;
    private final String correlationId;
    private final String causationId;
    private final String deduplicationId;
    private final String messageGroup;
    /* FlowCatalyst client (by code) this event belongs to; resolved to a client_id at ingest. */
    private final String clientCode;
    private final List<ContextEntry> contextData;
    private final Map<String, String> headers;

    private CreateEventDto(
            String type,
            Map<String, Object> data,
            String source,
            String subject,
            String correlationId,
            String causationId,
            String deduplicationId,
            String messageGroup,
            String clientCode,
            List<ContextEntry> contextData,
            Map<String, String> headers) {
        this.type = type;
        this.data = data;
        this.source = source;
        this.subject = subject;
        this.correlationId = correlationId;
        this.causationId = causationId;
        this.deduplicationId = deduplicationId;
        this.messageGroup = messageGroup;
        this.clientCode = clientCode;
        this.contextData = contextData;
        this.headers = headers;
    }

    /**
     * @param type fully qualified {@code application:subdomain:aggregate:action}
     *             event type; bare codes are rejected
     */
    public static CreateEventDto create(String type, Map<String, Object> data) {
        QualifiedCode.assertQualified(type, "Event type");
        return new CreateEventDto(
                type, data, null, null, null, null, null, null, null, List.of(), Map.of());
    }

    public CreateEventDto withSource(String source) {
        return new CreateEventDto(type, data, source, subject, correlationId, causationId,
                deduplicationId, messageGroup, clientCode, contextData, headers);
    }

    public CreateEventDto withSubject(String subject) {
        return new CreateEventDto(type, data, source, subject, correlationId, causationId,
                deduplicationId, messageGroup, clientCode, contextData, headers);
    }

    public CreateEventDto withCorrelationId(String correlationId) {
        return new CreateEventDto(type, data, source, subject, correlationId, causationId,
                deduplicationId, messageGroup, clientCode, contextData, headers);
    }

    public CreateEventDto withCausationId(String causationId) {
        return new CreateEventDto(type, data, source, subject, correlationId, causationId,
                deduplicationId, messageGroup, clientCode, contextData, headers);
    }

    public CreateEventDto withDeduplicationId(String deduplicationId) {
        return new CreateEventDto(type, data, source, subject, correlationId, causationId,
                deduplicationId, messageGroup, clientCode, contextData, headers);
    }

    public CreateEventDto withMessageGroup(String messageGroup) {
        return new CreateEventDto(type, data, source, subject, correlationId, causationId,
                deduplicationId, messageGroup, clientCode, contextData, headers);
    }

    /** Set the FlowCatalyst client (by code) this event belongs to. */
    public CreateEventDto withClientCode(String clientCode) {
        return new CreateEventDto(type, data, source, subject, correlationId, causationId,
                deduplicationId, messageGroup, clientCode, contextData, headers);
    }

    /** Merge additional headers (existing keys are overwritten). */
    public CreateEventDto withHeaders(Map<String, String> extra) {
        Map<String, String> merged = new LinkedHashMap<>(headers);
        merged.putAll(extra);
        return new CreateEventDto(type, data, source, subject, correlationId, causationId,
                deduplicationId, messageGroup, clientCode, contextData, Map.copyOf(merged));
    }

    /** Append context-data entries. */
    public CreateEventDto withContextData(List<ContextEntry> extra) {
        List<ContextEntry> merged = new ArrayList<>(contextData);
        merged.addAll(extra);
        return new CreateEventDto(type, data, source, subject, correlationId, causationId,
                deduplicationId, messageGroup, clientCode, List.copyOf(merged), headers);
    }

    public String messageGroup() {
        return messageGroup;
    }

    public Map<String, String> headers() {
        return headers;
    }

    /**
     * Build the event payload for the outbox (nulls omitted). {@code data} is
     * embedded as a JSON string, matching the wire contract.
     */
    Map<String, Object> toPayload(com.fasterxml.jackson.databind.ObjectMapper mapper) {
        Map<String, Object> payload = new LinkedHashMap<>();
        payload.put("specVersion", "1.0");
        payload.put("type", type);
        putIfNotNull(payload, "source", source);
        putIfNotNull(payload, "subject", subject);
        try {
            payload.put("data", mapper.writeValueAsString(data));
        } catch (com.fasterxml.jackson.core.JsonProcessingException e) {
            throw new IllegalArgumentException("Event data is not serializable to JSON", e);
        }
        putIfNotNull(payload, "correlationId", correlationId);
        putIfNotNull(payload, "causationId", causationId);
        putIfNotNull(payload, "deduplicationId", deduplicationId);
        putIfNotNull(payload, "messageGroup", messageGroup);
        putIfNotNull(payload, "clientCode", clientCode);
        if (!contextData.isEmpty()) {
            payload.put("contextData", contextData);
        }
        return payload;
    }

    private static void putIfNotNull(Map<String, Object> map, String key, Object value) {
        if (value != null) map.put(key, value);
    }
}
