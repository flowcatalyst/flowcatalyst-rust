package io.flowcatalyst.sdk.resources;

import io.flowcatalyst.sdk.generated.model.PermissionListResponse;
import io.flowcatalyst.sdk.generated.model.PermissionResponse;
import io.flowcatalyst.sdk.http.Transport;

/** Permissions resource — read the permission catalogue. */
public final class PermissionsResource {

    private final Transport transport;

    public PermissionsResource(Transport transport) {
        this.transport = transport;
    }

    public PermissionListResponse list() {
        return transport.get("/api/roles/permissions", null, PermissionListResponse.class);
    }

    public PermissionResponse get(String permission) {
        return transport.get(
                "/api/roles/permissions/" + Transport.enc(permission), null, PermissionResponse.class);
    }
}
