package io.flowcatalyst.sdk.resources;

import io.flowcatalyst.sdk.generated.model.AddSchemaRequest;
import io.flowcatalyst.sdk.generated.model.CreateEventTypeRequest;
import io.flowcatalyst.sdk.generated.model.CreatedResponse;
import io.flowcatalyst.sdk.generated.model.EventTypeListResponse;
import io.flowcatalyst.sdk.generated.model.EventTypeResponse;
import io.flowcatalyst.sdk.generated.model.SyncEventTypesRequest;
import io.flowcatalyst.sdk.generated.model.SyncResultResponse;
import io.flowcatalyst.sdk.generated.model.UpdateEventTypeRequest;
import io.flowcatalyst.sdk.http.Transport;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/** Event Types resource — manage event type definitions and schemas. */
public final class EventTypesResource {

    private final Transport transport;

    public EventTypesResource(Transport transport) {
        this.transport = transport;
    }

    /** Optional filters for {@link #list}. */
    public record Filters(
            String status, String application, String clientId, String subdomain, String aggregate) {
        public static Filters none() {
            return new Filters(null, null, null, null, null);
        }
    }

    /** List all event types with optional filters. */
    public EventTypeListResponse list(Filters filters) {
        Map<String, Object> query = new LinkedHashMap<>();
        if (filters != null) {
            query.put("status", filters.status());
            query.put("application", filters.application());
            query.put("clientId", filters.clientId());
            query.put("subdomain", filters.subdomain());
            query.put("aggregate", filters.aggregate());
        }
        return transport.get("/api/event-types", query, EventTypeListResponse.class);
    }

    /** Get an event type by ID. */
    public EventTypeResponse get(String id) {
        return transport.get("/api/event-types/" + Transport.enc(id), null, EventTypeResponse.class);
    }

    /** Get an event type by code. */
    public EventTypeResponse getByCode(String code) {
        return transport.get(
                "/api/event-types/by-code/" + Transport.enc(code), null, EventTypeResponse.class);
    }

    /** Create a new event type. */
    public CreatedResponse create(CreateEventTypeRequest data) {
        return transport.post("/api/event-types", data, CreatedResponse.class);
    }

    /** Update an event type. */
    public void update(String id, UpdateEventTypeRequest data) {
        transport.put("/api/event-types/" + Transport.enc(id), data, Void.class);
    }

    /** Add a schema version to an event type. */
    public EventTypeResponse addSchemaVersion(String id, AddSchemaRequest schema) {
        return transport.post(
                "/api/event-types/" + Transport.enc(id) + "/schemas", schema, EventTypeResponse.class);
    }

    /**
     * Archive (soft-delete) an event type. The server's DELETE on this
     * resource is a soft archive — the row is retained with status flipped to
     * ARCHIVED. Named {@code archive} rather than {@code delete} to make the
     * semantics visible (the TypeScript and Laravel SDKs match).
     */
    public void archive(String id) {
        transport.delete("/api/event-types/" + Transport.enc(id), Void.class);
    }

    /**
     * Sync event types for an application.
     * Calls {@code POST /api/applications/{appCode}/event-types/sync}.
     */
    public SyncResultResponse sync(
            String applicationCode,
            List<io.flowcatalyst.sdk.generated.model.SyncEventTypeInputRequest> eventTypes,
            boolean removeUnlisted) {
        SyncEventTypesRequest body = new SyncEventTypesRequest().eventTypes(eventTypes);
        return transport.post(
                "/api/applications/" + Transport.enc(applicationCode) + "/event-types/sync",
                Map.of("removeUnlisted", removeUnlisted),
                body,
                SyncResultResponse.class);
    }
}
