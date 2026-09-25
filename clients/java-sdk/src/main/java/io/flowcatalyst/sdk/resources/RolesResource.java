package io.flowcatalyst.sdk.resources;

import io.flowcatalyst.sdk.generated.model.CreateRoleRequest;
import io.flowcatalyst.sdk.generated.model.CreatedResponse;
import io.flowcatalyst.sdk.generated.model.GrantPermissionRequest;
import io.flowcatalyst.sdk.generated.model.RoleListResponse;
import io.flowcatalyst.sdk.generated.model.RoleResponse;
import io.flowcatalyst.sdk.generated.model.SyncResultResponse;
import io.flowcatalyst.sdk.generated.model.SyncRoleInputRequest;
import io.flowcatalyst.sdk.generated.model.SyncRolesRequest;
import io.flowcatalyst.sdk.generated.model.UpdateRoleRequest;
import io.flowcatalyst.sdk.http.Transport;
import java.util.List;
import java.util.Map;

/** Roles resource — manage roles and their permissions. */
public final class RolesResource {

    private final Transport transport;

    public RolesResource(Transport transport) {
        this.transport = transport;
    }

    public RoleListResponse list() {
        return transport.get("/api/roles", null, RoleListResponse.class);
    }

    /** Get a role by name (role names are the ID axis for roles). */
    public RoleResponse get(String roleName) {
        return transport.get("/api/roles/" + Transport.enc(roleName), null, RoleResponse.class);
    }

    public RoleResponse getByCode(String code) {
        return transport.get("/api/roles/by-code/" + Transport.enc(code), null, RoleResponse.class);
    }

    public CreatedResponse create(CreateRoleRequest data) {
        return transport.post("/api/roles", data, CreatedResponse.class);
    }

    public void update(String roleName, UpdateRoleRequest data) {
        transport.put("/api/roles/" + Transport.enc(roleName), data, Void.class);
    }

    public void delete(String roleName) {
        transport.delete("/api/roles/" + Transport.enc(roleName), Void.class);
    }

    /** List roles defined for an application. */
    public List<RoleResponse> listForApplication(String applicationId) {
        return transport.get(
                "/api/roles/by-application/" + Transport.enc(applicationId),
                null,
                transport.listOf(RoleResponse.class));
    }

    public RoleResponse grantPermission(String roleName, String permission) {
        GrantPermissionRequest body = new GrantPermissionRequest().permission(permission);
        return transport.post(
                "/api/roles/" + Transport.enc(roleName) + "/permissions", body, RoleResponse.class);
    }

    public RoleResponse revokePermission(String roleName, String permission) {
        return transport.delete(
                "/api/roles/" + Transport.enc(roleName) + "/permissions/" + Transport.enc(permission),
                RoleResponse.class);
    }

    /**
     * Sync roles for an application.
     * Calls {@code POST /api/applications/{appCode}/roles/sync}.
     */
    public SyncResultResponse sync(
            String applicationCode, List<SyncRoleInputRequest> roles, boolean removeUnlisted) {
        SyncRolesRequest body = new SyncRolesRequest().roles(roles);
        return transport.post(
                "/api/applications/" + Transport.enc(applicationCode) + "/roles/sync",
                Map.of("removeUnlisted", removeUnlisted),
                body,
                SyncResultResponse.class);
    }
}
