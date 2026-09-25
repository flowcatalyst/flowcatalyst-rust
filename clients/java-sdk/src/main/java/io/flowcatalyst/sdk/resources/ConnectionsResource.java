package io.flowcatalyst.sdk.resources;

import io.flowcatalyst.sdk.generated.model.ConnectionListResponse;
import io.flowcatalyst.sdk.generated.model.ConnectionResponse;
import io.flowcatalyst.sdk.generated.model.CreateConnectionRequest;
import io.flowcatalyst.sdk.generated.model.UpdateConnectionRequest;
import io.flowcatalyst.sdk.http.Transport;
import java.util.LinkedHashMap;
import java.util.Map;

/** Connections resource — manage delivery connections (HTTP targets, queues). */
public final class ConnectionsResource {

    private final Transport transport;

    public ConnectionsResource(Transport transport) {
        this.transport = transport;
    }

    /** List connections, optionally filtered by status and/or client. */
    public ConnectionListResponse list(String status, String clientId) {
        Map<String, Object> query = new LinkedHashMap<>();
        query.put("status", status);
        query.put("clientId", clientId);
        return transport.get("/api/connections", query, ConnectionListResponse.class);
    }

    public ConnectionResponse get(String id) {
        return transport.get("/api/connections/" + Transport.enc(id), null, ConnectionResponse.class);
    }

    public ConnectionResponse create(CreateConnectionRequest data) {
        return transport.post("/api/connections", data, ConnectionResponse.class);
    }

    public void update(String id, UpdateConnectionRequest data) {
        transport.put("/api/connections/" + Transport.enc(id), data, Void.class);
    }

    public void delete(String id) {
        transport.delete("/api/connections/" + Transport.enc(id), Void.class);
    }

    public ConnectionResponse pause(String id) {
        return transport.post(
                "/api/connections/" + Transport.enc(id) + "/pause", null, ConnectionResponse.class);
    }

    public ConnectionResponse activate(String id) {
        return transport.post(
                "/api/connections/" + Transport.enc(id) + "/activate", null, ConnectionResponse.class);
    }
}
