package io.flowcatalyst.sdk.resources;

import io.flowcatalyst.sdk.generated.model.AuditLogListResponse;
import io.flowcatalyst.sdk.generated.model.AuditLogResponse;
import io.flowcatalyst.sdk.http.Transport;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/** Audit Logs resource — read-only queries against the audit trail. */
public final class AuditLogsResource {

    private final Transport transport;

    public AuditLogsResource(Transport transport) {
        this.transport = transport;
    }

    /** Optional filters for {@link #list}; {@code after} is a cursor. */
    public record Filters(
            String after,
            Integer pageSize,
            String entityType,
            String entityId,
            String principalId,
            String operation,
            List<String> applicationIds,
            List<String> clientIds) {
        public static Filters none() {
            return new Filters(null, null, null, null, null, null, null, null);
        }
    }

    public AuditLogListResponse list(Filters filters) {
        Map<String, Object> query = new LinkedHashMap<>();
        if (filters != null) {
            query.put("after", filters.after());
            query.put("pageSize", filters.pageSize());
            query.put("entityType", filters.entityType());
            query.put("entityId", filters.entityId());
            query.put("principalId", filters.principalId());
            query.put("operation", filters.operation());
            query.put("applicationIds", filters.applicationIds());
            query.put("clientIds", filters.clientIds());
        }
        return transport.get("/api/audit-logs", query, AuditLogListResponse.class);
    }

    public AuditLogResponse get(String id) {
        return transport.get("/api/audit-logs/" + Transport.enc(id), null, AuditLogResponse.class);
    }

    public AuditLogListResponse recent() {
        return transport.get("/api/audit-logs/recent", null, AuditLogListResponse.class);
    }

    public AuditLogListResponse forEntity(String entityType, String entityId) {
        return transport.get(
                "/api/audit-logs/entity/" + Transport.enc(entityType) + "/"
                        + Transport.enc(entityId),
                null,
                AuditLogListResponse.class);
    }

    public AuditLogListResponse forPrincipal(String principalId) {
        return transport.get(
                "/api/audit-logs/principal/" + Transport.enc(principalId),
                null,
                AuditLogListResponse.class);
    }
}
