package io.flowcatalyst.sdk.resources;

import io.flowcatalyst.sdk.generated.model.AddNoteRequest;
import io.flowcatalyst.sdk.generated.model.ClientApplicationsResponse;
import io.flowcatalyst.sdk.generated.model.ClientListResponse;
import io.flowcatalyst.sdk.generated.model.ClientResponse;
import io.flowcatalyst.sdk.generated.model.CreateClientRequest;
import io.flowcatalyst.sdk.generated.model.CreatedResponse;
import io.flowcatalyst.sdk.generated.model.StatusChangeRequest;
import io.flowcatalyst.sdk.generated.model.StatusChangeResponse;
import io.flowcatalyst.sdk.generated.model.SuspendClientRequest;
import io.flowcatalyst.sdk.generated.model.UpdateClientApplicationsRequest;
import io.flowcatalyst.sdk.generated.model.UpdateClientRequest;
import io.flowcatalyst.sdk.http.Transport;
import java.util.Map;

/** Clients resource — tenants and their application enablement. */
public final class ClientsResource {

    private final Transport transport;

    public ClientsResource(Transport transport) {
        this.transport = transport;
    }

    public ClientListResponse list() {
        return transport.get("/api/clients", null, ClientListResponse.class);
    }

    public ClientResponse get(String id) {
        return transport.get("/api/clients/" + Transport.enc(id), null, ClientResponse.class);
    }

    public ClientResponse getByIdentifier(String identifier) {
        return transport.get(
                "/api/clients/by-identifier/" + Transport.enc(identifier), null,
                ClientResponse.class);
    }

    public CreatedResponse create(CreateClientRequest data) {
        return transport.post("/api/clients", data, CreatedResponse.class);
    }

    public void update(String id, UpdateClientRequest data) {
        transport.put("/api/clients/" + Transport.enc(id), data, Void.class);
    }

    public StatusChangeResponse activate(String id) {
        return transport.post(
                "/api/clients/" + Transport.enc(id) + "/activate", null, StatusChangeResponse.class);
    }

    public StatusChangeResponse deactivate(String id, String reason) {
        StatusChangeRequest body = new StatusChangeRequest().reason(reason);
        return transport.post(
                "/api/clients/" + Transport.enc(id) + "/deactivate", body,
                StatusChangeResponse.class);
    }

    public StatusChangeResponse suspend(String id, String reason) {
        SuspendClientRequest body = new SuspendClientRequest().reason(reason);
        return transport.post(
                "/api/clients/" + Transport.enc(id) + "/suspend", body, StatusChangeResponse.class);
    }

    /** Applications enabled for a client. */
    public ClientApplicationsResponse getApplications(String id) {
        return transport.get(
                "/api/clients/" + Transport.enc(id) + "/applications",
                null,
                ClientApplicationsResponse.class);
    }

    /** Replace the set of applications enabled for a client. */
    public void updateApplications(String id, UpdateClientApplicationsRequest data) {
        transport.put("/api/clients/" + Transport.enc(id) + "/applications", data, Void.class);
    }

    public void enableApplication(String clientId, String applicationId) {
        transport.post(
                "/api/clients/" + Transport.enc(clientId) + "/applications/"
                        + Transport.enc(applicationId) + "/enable",
                null,
                Void.class);
    }

    public void disableApplication(String clientId, String applicationId) {
        transport.post(
                "/api/clients/" + Transport.enc(clientId) + "/applications/"
                        + Transport.enc(applicationId) + "/disable",
                null,
                Void.class);
    }

    /** Search clients by free-text query. */
    public ClientListResponse search(String query) {
        return transport.get("/api/clients/search", Map.of("q", query), ClientListResponse.class);
    }

    public StatusChangeResponse addNote(String id, String category, String text) {
        AddNoteRequest body = new AddNoteRequest().category(category).text(text);
        return transport.post(
                "/api/clients/" + Transport.enc(id) + "/notes", body, StatusChangeResponse.class);
    }
}
