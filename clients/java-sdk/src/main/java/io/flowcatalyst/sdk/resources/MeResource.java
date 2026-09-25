package io.flowcatalyst.sdk.resources;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;
import io.flowcatalyst.sdk.http.Transport;
import java.util.List;

/**
 * Me resource — user-scoped access to clients and applications the
 * authenticated user can see. These endpoints do NOT require admin
 * permissions and are not part of the published OpenAPI spec, so their
 * response types are defined here.
 */
public final class MeResource {

    private final Transport transport;

    public MeResource(Transport transport) {
        this.transport = transport;
    }

    /** Client the user has access to. */
    @JsonIgnoreProperties(ignoreUnknown = true)
    public record MyClient(
            String id,
            String name,
            String identifier,
            String status,
            String createdAt,
            String updatedAt) {}

    @JsonIgnoreProperties(ignoreUnknown = true)
    public record MyClientsResponse(List<MyClient> clients, int total) {}

    /** Application enabled for a client. */
    @JsonIgnoreProperties(ignoreUnknown = true)
    public record MyApplication(
            String id,
            String code,
            String name,
            String description,
            String iconUrl,
            String baseUrl,
            String website,
            String logoMimeType) {}

    @JsonIgnoreProperties(ignoreUnknown = true)
    public record MyApplicationsResponse(
            List<MyApplication> applications, int total, String clientId) {}

    /**
     * Clients the authenticated user has access to. Access is determined by
     * user scope: ANCHOR sees all active clients, PARTNER sees IdP-granted
     * clients plus explicit grants, CLIENT sees the home client plus IdP
     * additional clients and explicit grants.
     */
    public MyClientsResponse getClients() {
        return transport.get("/api/me/clients", null, MyClientsResponse.class);
    }

    /** Get a specific client the user has access to. */
    public MyClient getClient(String clientId) {
        return transport.get("/api/me/clients/" + Transport.enc(clientId), null, MyClient.class);
    }

    /** Applications enabled for a client the user has access to. */
    public MyApplicationsResponse getClientApplications(String clientId) {
        return transport.get(
                "/api/me/clients/" + Transport.enc(clientId) + "/applications",
                null,
                MyApplicationsResponse.class);
    }
}
