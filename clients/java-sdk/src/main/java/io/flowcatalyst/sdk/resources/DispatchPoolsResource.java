package io.flowcatalyst.sdk.resources;

import io.flowcatalyst.sdk.generated.model.CreateDispatchPoolRequest;
import io.flowcatalyst.sdk.generated.model.CreatedResponse;
import io.flowcatalyst.sdk.generated.model.DispatchPoolListResponse;
import io.flowcatalyst.sdk.generated.model.DispatchPoolResponse;
import io.flowcatalyst.sdk.generated.model.SyncDispatchPoolInputRequest;
import io.flowcatalyst.sdk.generated.model.SyncDispatchPoolsRequest;
import io.flowcatalyst.sdk.generated.model.SyncResultResponse;
import io.flowcatalyst.sdk.generated.model.UpdateDispatchPoolRequest;
import io.flowcatalyst.sdk.http.Transport;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/** Dispatch Pools resource — rate-limit / concurrency pools for delivery. */
public final class DispatchPoolsResource {

    private final Transport transport;

    public DispatchPoolsResource(Transport transport) {
        this.transport = transport;
    }

    /** List dispatch pools, optionally filtered by status and/or client. */
    public DispatchPoolListResponse list(String status, String clientId) {
        Map<String, Object> query = new LinkedHashMap<>();
        query.put("status", status);
        query.put("clientId", clientId);
        return transport.get("/api/dispatch-pools", query, DispatchPoolListResponse.class);
    }

    public DispatchPoolResponse get(String id) {
        return transport.get(
                "/api/dispatch-pools/" + Transport.enc(id), null, DispatchPoolResponse.class);
    }

    public CreatedResponse create(CreateDispatchPoolRequest data) {
        return transport.post("/api/dispatch-pools", data, CreatedResponse.class);
    }

    public void update(String id, UpdateDispatchPoolRequest data) {
        transport.put("/api/dispatch-pools/" + Transport.enc(id), data, Void.class);
    }

    public void delete(String id) {
        transport.delete("/api/dispatch-pools/" + Transport.enc(id), Void.class);
    }

    public void suspend(String id) {
        transport.post("/api/dispatch-pools/" + Transport.enc(id) + "/suspend", null, Void.class);
    }

    public void activate(String id) {
        transport.post("/api/dispatch-pools/" + Transport.enc(id) + "/activate", null, Void.class);
    }

    /**
     * Sync dispatch pools for an application.
     * Calls {@code POST /api/applications/{appCode}/dispatch-pools/sync}.
     */
    public SyncResultResponse sync(
            String applicationCode, List<SyncDispatchPoolInputRequest> pools, boolean removeUnlisted) {
        SyncDispatchPoolsRequest body = new SyncDispatchPoolsRequest().pools(pools);
        return transport.post(
                "/api/applications/" + Transport.enc(applicationCode) + "/dispatch-pools/sync",
                Map.of("removeUnlisted", removeUnlisted),
                body,
                SyncResultResponse.class);
    }
}
