package io.flowcatalyst.sdk.outbox;

import java.time.Instant;
import java.util.LinkedHashMap;
import java.util.Map;

/**
 * DTO for creating an audit log entry in the outbox. Immutable builder — all
 * {@code with*} methods return a new instance.
 *
 * <pre>{@code
 * var auditLog = CreateAuditLogDto
 *         .create("User", "0HZXEQ5Y8JY5Z", "CREATE")
 *         .withOperationData(Map.of("email", "user@example.com"))
 *         .withPrincipalId("0HZXEQ5Y8JY5A")
 *         .withSource("user-service");
 * }</pre>
 */
public final class CreateAuditLogDto {

    private String entityType;
    private String entityId;
    private String operation;
    private Map<String, Object> operationData;
    private String principalId;
    private Instant performedAt;
    private String source;
    private String correlationId;
    /* FlowCatalyst application (by code); resolved to an application_id at ingest. */
    private String applicationCode;
    /* FlowCatalyst client (by code); resolved to a client_id at ingest. */
    private String clientCode;
    private Map<String, String> metadata = Map.of();
    private Map<String, String> headers = Map.of();

    private CreateAuditLogDto() {}

    private CreateAuditLogDto copy() {
        CreateAuditLogDto c = new CreateAuditLogDto();
        c.entityType = entityType;
        c.entityId = entityId;
        c.operation = operation;
        c.operationData = operationData;
        c.principalId = principalId;
        c.performedAt = performedAt;
        c.source = source;
        c.correlationId = correlationId;
        c.applicationCode = applicationCode;
        c.clientCode = clientCode;
        c.metadata = metadata;
        c.headers = headers;
        return c;
    }

    public static CreateAuditLogDto create(String entityType, String entityId, String operation) {
        CreateAuditLogDto dto = new CreateAuditLogDto();
        dto.entityType = entityType;
        dto.entityId = entityId;
        dto.operation = operation;
        return dto;
    }

    public CreateAuditLogDto withOperationData(Map<String, Object> operationData) {
        CreateAuditLogDto c = copy();
        c.operationData = operationData;
        return c;
    }

    public CreateAuditLogDto withPrincipalId(String principalId) {
        CreateAuditLogDto c = copy();
        c.principalId = principalId;
        return c;
    }

    public CreateAuditLogDto withPerformedAt(Instant performedAt) {
        CreateAuditLogDto c = copy();
        c.performedAt = performedAt;
        return c;
    }

    public CreateAuditLogDto withSource(String source) {
        CreateAuditLogDto c = copy();
        c.source = source;
        return c;
    }

    public CreateAuditLogDto withCorrelationId(String correlationId) {
        CreateAuditLogDto c = copy();
        c.correlationId = correlationId;
        return c;
    }

    /** Set the FlowCatalyst application (by code) this audit log belongs to. */
    public CreateAuditLogDto withApplicationCode(String applicationCode) {
        CreateAuditLogDto c = copy();
        c.applicationCode = applicationCode;
        return c;
    }

    /** Set the FlowCatalyst client (by code) this audit log belongs to. */
    public CreateAuditLogDto withClientCode(String clientCode) {
        CreateAuditLogDto c = copy();
        c.clientCode = clientCode;
        return c;
    }

    /** Merge additional metadata (existing keys are overwritten). */
    public CreateAuditLogDto withMetadata(Map<String, String> extra) {
        CreateAuditLogDto c = copy();
        Map<String, String> merged = new LinkedHashMap<>(metadata);
        merged.putAll(extra);
        c.metadata = Map.copyOf(merged);
        return c;
    }

    /** Merge additional headers (existing keys are overwritten). */
    public CreateAuditLogDto withHeaders(Map<String, String> extra) {
        CreateAuditLogDto c = copy();
        Map<String, String> merged = new LinkedHashMap<>(headers);
        merged.putAll(extra);
        c.headers = Map.copyOf(merged);
        return c;
    }

    public Map<String, String> headers() {
        return headers;
    }

    /**
     * Build the audit log payload for the outbox (nulls omitted).
     * {@code operationData} is embedded as a JSON string; {@code performedAt}
     * defaults to now.
     */
    Map<String, Object> toPayload(com.fasterxml.jackson.databind.ObjectMapper mapper) {
        Map<String, Object> payload = new LinkedHashMap<>();
        payload.put("entityType", entityType);
        payload.put("entityId", entityId);
        payload.put("operation", operation);
        if (operationData != null) {
            try {
                payload.put("operationData", mapper.writeValueAsString(operationData));
            } catch (com.fasterxml.jackson.core.JsonProcessingException e) {
                throw new IllegalArgumentException(
                        "Audit operationData is not serializable to JSON", e);
            }
        }
        putIfNotNull(payload, "principalId", principalId);
        payload.put("performedAt", (performedAt != null ? performedAt : Instant.now()).toString());
        putIfNotNull(payload, "source", source);
        putIfNotNull(payload, "correlationId", correlationId);
        putIfNotNull(payload, "applicationCode", applicationCode);
        putIfNotNull(payload, "clientCode", clientCode);
        if (!metadata.isEmpty()) payload.put("metadata", metadata);
        return payload;
    }

    private static void putIfNotNull(Map<String, Object> map, String key, Object value) {
        if (value != null) map.put(key, value);
    }
}
