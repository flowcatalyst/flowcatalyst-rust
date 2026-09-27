package io.flowcatalyst.sdk.resources;

import io.flowcatalyst.sdk.error.FlowCatalystException;
import io.flowcatalyst.sdk.error.SdkError;
import io.flowcatalyst.sdk.generated.model.ApplicationListResponse;
import io.flowcatalyst.sdk.generated.model.ApplicationProvisionServiceAccountResponse;
import io.flowcatalyst.sdk.generated.model.ApplicationResponse;
import io.flowcatalyst.sdk.generated.model.ApplicationRolesResponse;
import io.flowcatalyst.sdk.generated.model.ClientConfigListResponse;
import io.flowcatalyst.sdk.generated.model.ClientConfigResponse;
import io.flowcatalyst.sdk.generated.model.CreateApplicationRequest;
import io.flowcatalyst.sdk.generated.model.CreatedResponse;
import io.flowcatalyst.sdk.generated.model.ServiceAccountResponse;
import io.flowcatalyst.sdk.generated.model.UpdateApplicationRequest;
import io.flowcatalyst.sdk.http.Transport;
import java.util.LinkedHashMap;
import java.util.Map;

/** Applications resource — registered applications and per-client enablement. */
public final class ApplicationsResource {

    private final Transport transport;

    public ApplicationsResource(Transport transport) {
        this.transport = transport;
    }

    /** Per-client application config update payload (record fields are nullable = omitted). */
    public record ClientConfigRequest(
            Boolean enabled, String baseUrlOverride, Map<String, Object> config) {}

    public ApplicationListResponse list() {
        return transport.get("/api/applications", null, ApplicationListResponse.class);
    }

    /**
     * List applications, filtered by {@code type} ({@code APPLICATION} or
     * {@code INTEGRATION}) and/or {@code active}; a null filter is not sent.
     */
    public ApplicationListResponse list(String type, Boolean active) {
        Map<String, Object> query = new LinkedHashMap<>();
        query.put("type", type);
        query.put("active", active);
        return transport.get("/api/applications", query, ApplicationListResponse.class);
    }

    public ApplicationResponse get(String id) {
        return transport.get(
                "/api/applications/" + Transport.enc(id), null, ApplicationResponse.class);
    }

    public ApplicationResponse getByCode(String code) {
        return transport.get(
                "/api/applications/by-code/" + Transport.enc(code), null, ApplicationResponse.class);
    }

    public CreatedResponse create(CreateApplicationRequest data) {
        return transport.post("/api/applications", data, CreatedResponse.class);
    }

    public void update(String id, UpdateApplicationRequest data) {
        transport.put("/api/applications/" + Transport.enc(id), data, Void.class);
    }

    public void delete(String id) {
        transport.delete("/api/applications/" + Transport.enc(id), Void.class);
    }

    public ApplicationResponse activate(String id) {
        return transport.post(
                "/api/applications/" + Transport.enc(id) + "/activate", null,
                ApplicationResponse.class);
    }

    public ApplicationResponse deactivate(String id) {
        return transport.post(
                "/api/applications/" + Transport.enc(id) + "/deactivate", null,
                ApplicationResponse.class);
    }

    /** Provision a service account for an application. */
    public ApplicationProvisionServiceAccountResponse provisionServiceAccount(String id) {
        return transport.post(
                "/api/applications/" + Transport.enc(id) + "/provision-service-account",
                null,
                ApplicationProvisionServiceAccountResponse.class);
    }

    /**
     * Get the service account attached to an application.
     *
     * <p>The platform has no read route for an application's service account,
     * so this reads the application and then
     * {@code GET /api/service-accounts/{serviceAccountId}}.
     *
     * @throws FlowCatalystException with {@link SdkError.NotFound} when the
     *     application has no service account
     */
    public ServiceAccountResponse getServiceAccount(String id) {
        String serviceAccountId = get(id).getServiceAccountId();
        if (serviceAccountId == null || serviceAccountId.isEmpty()) {
            throw new FlowCatalystException(
                    new SdkError.NotFound("Application " + id + " has no service account"));
        }
        return transport.get(
                "/api/service-accounts/" + Transport.enc(serviceAccountId),
                null,
                ServiceAccountResponse.class);
    }

    /** List roles defined for an application. */
    public ApplicationRolesResponse listRoles(String id) {
        return transport.get(
                "/api/applications/by-id/" + Transport.enc(id) + "/roles",
                null,
                ApplicationRolesResponse.class);
    }

    /** List per-client configs for an application. */
    public ClientConfigListResponse listClients(String id) {
        return transport.get(
                "/api/applications/" + Transport.enc(id) + "/clients",
                null,
                ClientConfigListResponse.class);
    }

    /** Get one client's configuration for an application. */
    public ClientConfigResponse getClientConfig(String id, String clientId) {
        return transport.get(
                "/api/applications/" + Transport.enc(id) + "/clients/" + Transport.enc(clientId),
                null,
                ClientConfigResponse.class);
    }

    /**
     * Update the per-client config for an application.
     *
     * @deprecated Only the Rust platform serves
     *     {@code PUT /api/applications/{id}/clients/{clientId}}; the Go platform
     *     has no such route. Use {@link #enableForClient} /
     *     {@link #disableForClient}, and {@link #getClientConfig} to read the
     *     result.
     */
    @Deprecated
    public ClientConfigResponse updateClientConfig(
            String id, String clientId, ClientConfigRequest data) {
        return transport.put(
                "/api/applications/" + Transport.enc(id) + "/clients/" + Transport.enc(clientId),
                data,
                ClientConfigResponse.class);
    }

    /** Enable an application for a client. */
    public void enableForClient(String id, String clientId) {
        transport.post(
                "/api/applications/" + Transport.enc(id) + "/clients/" + Transport.enc(clientId)
                        + "/enable",
                null,
                Void.class);
    }

    /** Disable an application for a client. */
    public void disableForClient(String id, String clientId) {
        transport.post(
                "/api/applications/" + Transport.enc(id) + "/clients/" + Transport.enc(clientId)
                        + "/disable",
                null,
                Void.class);
    }
}
