package io.flowcatalyst.sdk.resources;

import io.flowcatalyst.sdk.generated.model.CreateProcessRequest;
import io.flowcatalyst.sdk.generated.model.CreatedResponse;
import io.flowcatalyst.sdk.generated.model.ProcessListResponse;
import io.flowcatalyst.sdk.generated.model.ProcessResponse;
import io.flowcatalyst.sdk.generated.model.SyncProcessInputRequest;
import io.flowcatalyst.sdk.generated.model.SyncProcessesRequest;
import io.flowcatalyst.sdk.generated.model.SyncResultResponse;
import io.flowcatalyst.sdk.generated.model.UpdateProcessRequest;
import io.flowcatalyst.sdk.http.Transport;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/** Processes resource — workflow documentation / Mermaid diagrams. */
public final class ProcessesResource {

    private final Transport transport;

    public ProcessesResource(Transport transport) {
        this.transport = transport;
    }

    /** List processes, optionally filtered by application, subdomain, status. */
    public ProcessListResponse list(String application, String subdomain, String status) {
        Map<String, Object> query = new LinkedHashMap<>();
        query.put("application", application);
        query.put("subdomain", subdomain);
        query.put("status", status);
        return transport.get("/api/processes", query, ProcessListResponse.class);
    }

    public ProcessResponse get(String id) {
        return transport.get("/api/processes/" + Transport.enc(id), null, ProcessResponse.class);
    }

    public ProcessResponse getByCode(String code) {
        return transport.get(
                "/api/processes/by-code/" + Transport.enc(code), null, ProcessResponse.class);
    }

    public CreatedResponse create(CreateProcessRequest data) {
        return transport.post("/api/processes", data, CreatedResponse.class);
    }

    public void update(String id, UpdateProcessRequest data) {
        transport.put("/api/processes/" + Transport.enc(id), data, Void.class);
    }

    public void archive(String id) {
        transport.post("/api/processes/" + Transport.enc(id) + "/archive", null, Void.class);
    }

    public void delete(String id) {
        transport.delete("/api/processes/" + Transport.enc(id), Void.class);
    }

    /**
     * Sync processes for an application.
     * Calls {@code POST /api/applications/{appCode}/processes/sync}.
     */
    public SyncResultResponse sync(
            String applicationCode, List<SyncProcessInputRequest> processes, boolean removeUnlisted) {
        SyncProcessesRequest body = new SyncProcessesRequest().processes(processes);
        return transport.post(
                "/api/applications/" + Transport.enc(applicationCode) + "/processes/sync",
                Map.of("removeUnlisted", removeUnlisted),
                body,
                SyncResultResponse.class);
    }
}
