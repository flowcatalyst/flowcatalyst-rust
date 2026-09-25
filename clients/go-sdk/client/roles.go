package client

import (
	"context"
	"fmt"
)

// ─── Request DTOs ────────────────────────────────────────────────────

// CreateRoleRequest — POST /api/roles. ClientManaged is required by the
// platform, so it is always sent (false included).
type CreateRoleRequest struct {
	ApplicationCode string   `json:"applicationCode"`
	RoleName        string   `json:"roleName"`
	DisplayName     string   `json:"displayName"`
	Description     string   `json:"description,omitempty"`
	Permissions     []string `json:"permissions,omitempty"`
	ClientManaged   bool     `json:"clientManaged"`
}

// UpdateRoleRequest — PUT /api/roles/{name}.
type UpdateRoleRequest struct {
	DisplayName   *string `json:"displayName,omitempty"`
	Description   *string `json:"description,omitempty"`
	ClientManaged *bool   `json:"clientManaged,omitempty"`
}

// GrantPermissionRequest — body for POST /api/roles/{name}/permissions.
type GrantPermissionRequest struct {
	Permission string `json:"permission"`
}

// ─── Response DTOs ───────────────────────────────────────────────────

// RoleResponse is the platform's role aggregate.
type RoleResponse struct {
	ID              string   `json:"id"`
	Name            string   `json:"name"`
	ShortName       string   `json:"shortName"`
	DisplayName     string   `json:"displayName"`
	Description     string   `json:"description,omitempty"`
	ApplicationCode string   `json:"applicationCode"`
	Permissions     []string `json:"permissions,omitempty"`
	Source          string   `json:"source"`
	ClientManaged   bool     `json:"clientManaged,omitempty"`
	CreatedAt       string   `json:"createdAt"`
	UpdatedAt       string   `json:"updatedAt"`
}

// RoleListResponse — GET /api/roles.
type RoleListResponse struct {
	Roles []RoleResponse `json:"roles"`
	Total uint64         `json:"total,omitempty"`
}

// ─── Sync ────────────────────────────────────────────────────────────

// SyncRoleItem — one entry in the per-app sync payload.
type SyncRoleItem struct {
	Name          string   `json:"name"`
	DisplayName   string   `json:"displayName,omitempty"`
	Description   string   `json:"description,omitempty"`
	Permissions   []string `json:"permissions,omitempty"`
	ClientManaged bool     `json:"clientManaged,omitempty"`
}

// SyncRolesRequest — body for the per-app sync endpoint.
type SyncRolesRequest struct {
	Roles []SyncRoleItem `json:"roles"`
}

// ─── Resource ────────────────────────────────────────────────────────

// RolesResource — /api/roles/*.
type RolesResource struct {
	c *FlowCatalystClient
}

// List — GET /api/roles.
func (r *RolesResource) List(ctx context.Context) (*RoleListResponse, error) {
	var out RoleListResponse
	if err := r.c.Get(ctx, "/api/roles", &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Get — GET /api/roles/{name}.
func (r *RolesResource) Get(ctx context.Context, name string) (*RoleResponse, error) {
	var out RoleResponse
	if err := r.c.Get(ctx, "/api/roles/"+name, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// GetByCode — GET /api/roles/by-code/{code} (`application:role-name`).
func (r *RolesResource) GetByCode(ctx context.Context, code string) (*RoleResponse, error) {
	var out RoleResponse
	if err := r.c.Get(ctx, "/api/roles/by-code/"+code, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Create — POST /api/roles. Returns { id } only; call Get(name) for
// the full record.
func (r *RolesResource) Create(ctx context.Context, req *CreateRoleRequest) (*CreatedResponse, error) {
	var out CreatedResponse
	if err := r.c.Post(ctx, "/api/roles", req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Update — PUT /api/roles/{name}. Platform responds with 204.
func (r *RolesResource) Update(ctx context.Context, name string, req *UpdateRoleRequest) error {
	return r.c.Put(ctx, "/api/roles/"+name, req, nil)
}

// Delete — DELETE /api/roles/{name}.
func (r *RolesResource) Delete(ctx context.Context, name string) error {
	return r.c.Delete(ctx, "/api/roles/"+name, nil)
}

// ListForApplication — GET /api/roles/by-application/{applicationId}.
// The platform answers a bare array of roles; it is returned wrapped in a
// RoleListResponse (Total = number of roles).
func (r *RolesResource) ListForApplication(ctx context.Context, applicationID string) (*RoleListResponse, error) {
	var roles []RoleResponse
	if err := r.c.Get(ctx, "/api/roles/by-application/"+applicationID, &roles); err != nil {
		return nil, err
	}
	return &RoleListResponse{Roles: roles, Total: uint64(len(roles))}, nil
}

// GrantPermission — POST /api/roles/{name}/permissions. Returns the updated role.
func (r *RolesResource) GrantPermission(ctx context.Context, roleName, permission string) (*RoleResponse, error) {
	var out RoleResponse
	if err := r.c.Post(ctx, "/api/roles/"+roleName+"/permissions",
		&GrantPermissionRequest{Permission: permission}, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// RevokePermission — DELETE /api/roles/{name}/permissions/{permission}. Returns the updated role.
func (r *RolesResource) RevokePermission(ctx context.Context, roleName, permission string) (*RoleResponse, error) {
	var out RoleResponse
	if err := r.c.Delete(ctx, fmt.Sprintf("/api/roles/%s/permissions/%s", roleName, permission), &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Sync — POST /api/applications/{appCode}/roles/sync.
func (r *RolesResource) Sync(ctx context.Context, appCode string, req *SyncRolesRequest, removeUnlisted bool) (*SyncResult, error) {
	q := ""
	if removeUnlisted {
		q = "?removeUnlisted=true"
	}
	var out SyncResult
	if err := r.c.Post(ctx, fmt.Sprintf("/api/applications/%s/roles/sync%s", appCode, q), req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}
