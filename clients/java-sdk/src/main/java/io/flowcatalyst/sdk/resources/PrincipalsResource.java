package io.flowcatalyst.sdk.resources;

import io.flowcatalyst.sdk.generated.model.AddRoleRequest;
import io.flowcatalyst.sdk.generated.model.AssignPrincipalRolesRequest;
import io.flowcatalyst.sdk.generated.model.ClientAccessGrantListResponse;
import io.flowcatalyst.sdk.generated.model.ClientAccessGrantResponse;
import io.flowcatalyst.sdk.generated.model.CreateUserRequest;
import io.flowcatalyst.sdk.generated.model.GrantClientAccessRequest;
import io.flowcatalyst.sdk.generated.model.PrincipalListResponse;
import io.flowcatalyst.sdk.generated.model.PrincipalResponse;
import io.flowcatalyst.sdk.generated.model.PrincipalRoleListResponse;
import io.flowcatalyst.sdk.generated.model.ResetPasswordRequest;
import io.flowcatalyst.sdk.generated.model.RolesAssignedResponse;
import io.flowcatalyst.sdk.generated.model.StatusChangeResponse;
import io.flowcatalyst.sdk.generated.model.SyncPrincipalInputRequest;
import io.flowcatalyst.sdk.generated.model.SyncPrincipalsRequest;
import io.flowcatalyst.sdk.generated.model.SyncResultResponse;
import io.flowcatalyst.sdk.generated.model.SyncUserInput;
import io.flowcatalyst.sdk.generated.model.SyncUsersRequest;
import io.flowcatalyst.sdk.generated.model.SyncUsersResponse;
import io.flowcatalyst.sdk.generated.model.UpdatePrincipalRequest;
import io.flowcatalyst.sdk.http.Transport;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;

/** Principals resource — users and service accounts. */
public final class PrincipalsResource {

    private final Transport transport;

    public PrincipalsResource(Transport transport) {
        this.transport = transport;
    }

    /** Optional filters for {@link #list}. */
    public record Filters(
            String type,
            String clientId,
            Boolean active,
            String q,
            List<String> roles,
            Integer page,
            Integer pageSize,
            String sortField,
            String sortOrder) {
        public static Filters none() {
            return new Filters(null, null, null, null, null, null, null, null, null);
        }

        public static Filters query(String q) {
            return new Filters(null, null, null, q, null, null, null, null, null);
        }
    }

    public PrincipalListResponse list(Filters filters) {
        Map<String, Object> query = new LinkedHashMap<>();
        if (filters != null) {
            query.put("type", filters.type());
            query.put("clientId", filters.clientId());
            query.put("active", filters.active());
            query.put("q", filters.q());
            query.put("roles", filters.roles());
            query.put("page", filters.page());
            query.put("pageSize", filters.pageSize());
            query.put("sortField", filters.sortField());
            query.put("sortOrder", filters.sortOrder());
        }
        return transport.get("/api/principals", query, PrincipalListResponse.class);
    }

    public PrincipalResponse get(String id) {
        return transport.get("/api/principals/" + Transport.enc(id), null, PrincipalResponse.class);
    }

    /**
     * Find principals whose email matches exactly (case-insensitive). The
     * platform's list search is a broad substring match, so this filters
     * server results down to exact-equality here — callers must not act on
     * the wrong principal.
     */
    public PrincipalListResponse listByEmail(String email) {
        String needle = email.toLowerCase(Locale.ROOT);
        PrincipalListResponse response = list(Filters.query(email));
        List<PrincipalResponse> exact = response.getPrincipals() == null
                ? List.of()
                : response.getPrincipals().stream()
                        .filter(p -> p.getEmail() != null
                                && p.getEmail().toLowerCase(Locale.ROOT).equals(needle))
                        .toList();
        PrincipalListResponse filtered = new PrincipalListResponse();
        filtered.setPrincipals(exact);
        filtered.setTotal((long) exact.size());
        return filtered;
    }

    /** Create a new user principal. */
    public PrincipalResponse createUser(CreateUserRequest data) {
        return transport.post("/api/principals/users", data, PrincipalResponse.class);
    }

    public PrincipalResponse update(String id, UpdatePrincipalRequest data) {
        return transport.put("/api/principals/" + Transport.enc(id), data, PrincipalResponse.class);
    }

    public StatusChangeResponse activate(String id) {
        return transport.post(
                "/api/principals/" + Transport.enc(id) + "/activate", null,
                StatusChangeResponse.class);
    }

    public StatusChangeResponse deactivate(String id) {
        return transport.post(
                "/api/principals/" + Transport.enc(id) + "/deactivate", null,
                StatusChangeResponse.class);
    }

    public StatusChangeResponse resetPassword(String id, ResetPasswordRequest data) {
        return transport.post(
                "/api/principals/" + Transport.enc(id) + "/reset-password", data,
                StatusChangeResponse.class);
    }

    public PrincipalRoleListResponse getRoles(String id) {
        return transport.get(
                "/api/principals/" + Transport.enc(id) + "/roles",
                null,
                PrincipalRoleListResponse.class);
    }

    public PrincipalResponse addRole(String id, String roleName) {
        AddRoleRequest body = new AddRoleRequest().role(roleName);
        return transport.post(
                "/api/principals/" + Transport.enc(id) + "/roles", body, PrincipalResponse.class);
    }

    public PrincipalResponse removeRole(String id, String roleName) {
        return transport.delete(
                "/api/principals/" + Transport.enc(id) + "/roles/" + Transport.enc(roleName),
                PrincipalResponse.class);
    }

    /** Replace the principal's full role set. */
    public RolesAssignedResponse setRoles(String id, List<String> roles) {
        AssignPrincipalRolesRequest body = new AssignPrincipalRolesRequest().roles(roles);
        return transport.put(
                "/api/principals/" + Transport.enc(id) + "/roles", body, RolesAssignedResponse.class);
    }

    public ClientAccessGrantListResponse getClientAccessGrants(String id) {
        return transport.get(
                "/api/principals/" + Transport.enc(id) + "/client-access",
                null,
                ClientAccessGrantListResponse.class);
    }

    public ClientAccessGrantResponse grantClientAccess(String id, String clientId) {
        GrantClientAccessRequest body = new GrantClientAccessRequest().clientId(clientId);
        return transport.post(
                "/api/principals/" + Transport.enc(id) + "/client-access", body,
                ClientAccessGrantResponse.class);
    }

    public void revokeClientAccess(String id, String clientId) {
        transport.delete(
                "/api/principals/" + Transport.enc(id) + "/client-access/" + Transport.enc(clientId),
                Void.class);
    }

    /**
     * Sync principals for an application.
     * Calls {@code POST /api/applications/{appCode}/principals/sync}.
     */
    public SyncResultResponse sync(
            String applicationCode,
            List<SyncPrincipalInputRequest> principals,
            boolean removeUnlisted) {
        SyncPrincipalsRequest body = new SyncPrincipalsRequest().principals(principals);
        return transport.post(
                "/api/applications/" + Transport.enc(applicationCode) + "/principals/sync",
                Map.of("removeUnlisted", removeUnlisted),
                body,
                SyncResultResponse.class);
    }

    /** Bulk-sync platform users. Calls {@code POST /api/principals/sync}. */
    public SyncUsersResponse syncUsers(List<SyncUserInput> principals) {
        SyncUsersRequest body = new SyncUsersRequest().principals(principals);
        return transport.post("/api/principals/sync", body, SyncUsersResponse.class);
    }
}
