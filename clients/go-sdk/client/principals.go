package client

import (
	"context"
	"fmt"
	"strings"
)

// ─── Request DTOs ────────────────────────────────────────────────────

// CreateUserRequest — POST /api/principals/users.
type CreateUserRequest struct {
	Email    string `json:"email"`
	Password string `json:"password,omitempty"`
	Name     string `json:"name"`
	ClientID string `json:"clientId,omitempty"`
	// EnforcePasswordComplexity: pass *false to skip the platform's
	// upper/lower/digit/special rules (2-char minimum still applies).
	// Use when your application enforces its own password policy.
	EnforcePasswordComplexity *bool `json:"enforcePasswordComplexity,omitempty"`
}

// UpdatePrincipalRequest — PUT /api/principals/{id}. Nil members are left
// unchanged.
type UpdatePrincipalRequest struct {
	Name   *string `json:"name,omitempty"`
	Active *bool   `json:"active,omitempty"`
	// Email is asserted against the stored email: a different value is
	// rejected, not treated as a rename.
	Email *string `json:"email,omitempty"`
}

// ResetPasswordRequest — POST /api/principals/{id}/reset-password.
type ResetPasswordRequest struct {
	NewPassword               string `json:"newPassword"`
	EnforcePasswordComplexity *bool  `json:"enforcePasswordComplexity,omitempty"`
}

// AssignRoleRequest — body for POST /api/principals/{id}/roles. The
// backend expects { role }, not { roleName }.
type AssignRoleRequest struct {
	Role string `json:"role"`
}

// ReplaceRolesRequest — body for PUT /api/principals/{id}/roles.
type ReplaceRolesRequest struct {
	Roles []string `json:"roles"`
}

// GrantClientAccessRequest — body for POST /api/principals/{id}/client-access.
type GrantClientAccessRequest struct {
	ClientID string `json:"clientId"`
}

// PrincipalFilters — query parameters for GET /api/principals. Empty
// members are omitted.
type PrincipalFilters struct {
	ClientID string
	// Type — USER or SERVICE.
	Type string
	// Active — "true" or "false"; empty returns both.
	Active string
	// Q — case-insensitive substring search across name and email.
	Q string
	// Roles — principals holding any of these role names.
	Roles []string
	// Page — 0-based page index.
	Page *uint32
	// PageSize — page size; nil returns every match.
	PageSize *uint32
	// SortField — name, email or createdAt (default createdAt).
	SortField string
	// SortOrder — asc or desc (default asc).
	SortOrder string
}

// ─── Response DTOs ───────────────────────────────────────────────────

// PrincipalResponse is the platform's principal aggregate.
type PrincipalResponse struct {
	ID               string   `json:"id"`
	PrincipalType    string   `json:"type"`
	Scope            string   `json:"scope"`
	ClientID         string   `json:"clientId,omitempty"`
	Name             string   `json:"name"`
	Active           bool     `json:"active"`
	Email            string   `json:"email,omitempty"`
	IdpType          string   `json:"idpType,omitempty"`
	Roles            []string `json:"roles,omitempty"`
	IsAnchorUser     bool     `json:"isAnchorUser,omitempty"`
	GrantedClientIDs []string `json:"grantedClientIds,omitempty"`
	CreatedAt        string   `json:"createdAt"`
	UpdatedAt        string   `json:"updatedAt"`
}

// PrincipalListResponse — GET /api/principals.
type PrincipalListResponse struct {
	Principals []PrincipalResponse `json:"principals"`
	Total      uint64              `json:"total,omitempty"`
}

// PrincipalRoleResponse — one item from GET /api/principals/{id}/roles.
// Matches the platform's RoleAssignmentDto.
type PrincipalRoleResponse struct {
	ID               string `json:"id"`
	RoleName         string `json:"roleName"`
	AssignmentSource string `json:"assignmentSource"`
	AssignedAt       string `json:"assignedAt"`
}

// PrincipalRoleListResponse — GET /api/principals/{id}/roles.
type PrincipalRoleListResponse struct {
	Roles []PrincipalRoleResponse `json:"roles"`
}

// ClientAccessGrantResponse — one item from GET /api/principals/{id}/client-access.
type ClientAccessGrantResponse struct {
	ID        string `json:"id"`
	ClientID  string `json:"clientId"`
	GrantedAt string `json:"grantedAt"`
	ExpiresAt string `json:"expiresAt,omitempty"`
}

// ClientAccessListResponse — GET /api/principals/{id}/client-access.
type ClientAccessListResponse struct {
	Grants []ClientAccessGrantResponse `json:"grants"`
}

// ─── Sync ────────────────────────────────────────────────────────────

// SyncPrincipalItem — one entry in the per-app sync payload. Matched by
// email. Role names omit the `<app>:` prefix — the platform adds it.
type SyncPrincipalItem struct {
	Email  string   `json:"email"`
	Name   string   `json:"name"`
	Roles  []string `json:"roles,omitempty"`
	Active bool     `json:"active"`
	// PasswordHash — optional pre-hashed password (bcrypt, argon2), stored
	// verbatim on a user the sync creates. Ignored for an existing user;
	// SyncResult.PasswordHashIgnored names those.
	PasswordHash string `json:"passwordHash,omitempty"`
}

// SyncPrincipalsRequest — body for the per-app sync endpoint.
type SyncPrincipalsRequest struct {
	Principals []SyncPrincipalItem `json:"principals"`
}

// ─── Resource ────────────────────────────────────────────────────────

// PrincipalsResource — /api/principals/*.
type PrincipalsResource struct {
	c *FlowCatalystClient
}

// CreateUser — POST /api/principals/users.
func (r *PrincipalsResource) CreateUser(ctx context.Context, req *CreateUserRequest) (*PrincipalResponse, error) {
	var out PrincipalResponse
	if err := r.c.Post(ctx, "/api/principals/users", req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// List — GET /api/principals with optional filters.
func (r *PrincipalsResource) List(ctx context.Context, filters *PrincipalFilters) (*PrincipalListResponse, error) {
	q := ""
	if filters != nil {
		q = NewQuery().
			String("clientId", filters.ClientID).
			String("type", filters.Type).
			String("active", filters.Active).
			String("q", filters.Q).
			String("roles", strings.Join(filters.Roles, ",")).
			Uint32("page", filters.Page).
			Uint32("pageSize", filters.PageSize).
			String("sortField", filters.SortField).
			String("sortOrder", filters.SortOrder).
			Encode()
	}
	var out PrincipalListResponse
	if err := r.c.Get(ctx, "/api/principals"+q, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Get — GET /api/principals/{id}.
func (r *PrincipalsResource) Get(ctx context.Context, id string) (*PrincipalResponse, error) {
	var out PrincipalResponse
	if err := r.c.Get(ctx, "/api/principals/"+id, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// FindByEmail — convenience over List. The platform has no exact email
// filter, so this searches with q=email (a substring match on name and
// email) and keeps only the principals whose email equals email
// (case-insensitive). Total is the number kept.
func (r *PrincipalsResource) FindByEmail(ctx context.Context, email string) (*PrincipalListResponse, error) {
	res, err := r.List(ctx, &PrincipalFilters{Q: email})
	if err != nil {
		return nil, err
	}
	want := strings.TrimSpace(email)
	matched := make([]PrincipalResponse, 0, 1)
	for _, p := range res.Principals {
		if strings.EqualFold(strings.TrimSpace(p.Email), want) {
			matched = append(matched, p)
		}
	}
	return &PrincipalListResponse{Principals: matched, Total: uint64(len(matched))}, nil
}

// Update — PUT /api/principals/{id}.
func (r *PrincipalsResource) Update(ctx context.Context, id string, req *UpdatePrincipalRequest) (*PrincipalResponse, error) {
	var out PrincipalResponse
	if err := r.c.Put(ctx, "/api/principals/"+id, req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Activate — POST /api/principals/{id}/activate. Platform returns
// { message } only; call Get(id) for the refreshed record.
func (r *PrincipalsResource) Activate(ctx context.Context, id string) error {
	return r.c.Post(ctx, "/api/principals/"+id+"/activate", nil, nil)
}

// Deactivate — POST /api/principals/{id}/deactivate.
func (r *PrincipalsResource) Deactivate(ctx context.Context, id string) error {
	return r.c.Post(ctx, "/api/principals/"+id+"/deactivate", nil, nil)
}

// Roles — GET /api/principals/{id}/roles.
func (r *PrincipalsResource) Roles(ctx context.Context, id string) (*PrincipalRoleListResponse, error) {
	var out PrincipalRoleListResponse
	if err := r.c.Get(ctx, "/api/principals/"+id+"/roles", &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// AddRole — POST /api/principals/{id}/roles (additive — keeps existing
// roles). Distinct from SetRoles which replaces the full set.
func (r *PrincipalsResource) AddRole(ctx context.Context, id, roleName string) error {
	return r.c.Post(ctx, "/api/principals/"+id+"/roles", &AssignRoleRequest{Role: roleName}, nil)
}

// RemoveRole — DELETE /api/principals/{id}/roles/{roleName}.
func (r *PrincipalsResource) RemoveRole(ctx context.Context, id, roleName string) error {
	return r.c.Delete(ctx, fmt.Sprintf("/api/principals/%s/roles/%s", id, roleName), nil)
}

// SetRoles — PUT /api/principals/{id}/roles. Replaces the full set.
func (r *PrincipalsResource) SetRoles(ctx context.Context, id string, roles []string) error {
	return r.c.Put(ctx, "/api/principals/"+id+"/roles", &ReplaceRolesRequest{Roles: roles}, nil)
}

// ClientAccessGrants — GET /api/principals/{id}/client-access.
func (r *PrincipalsResource) ClientAccessGrants(ctx context.Context, id string) (*ClientAccessListResponse, error) {
	var out ClientAccessListResponse
	if err := r.c.Get(ctx, "/api/principals/"+id+"/client-access", &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// GrantClientAccess — POST /api/principals/{id}/client-access.
func (r *PrincipalsResource) GrantClientAccess(ctx context.Context, principalID, clientID string) error {
	return r.c.Post(ctx, "/api/principals/"+principalID+"/client-access",
		&GrantClientAccessRequest{ClientID: clientID}, nil)
}

// RevokeClientAccess — DELETE /api/principals/{id}/client-access/{clientId}.
func (r *PrincipalsResource) RevokeClientAccess(ctx context.Context, principalID, clientID string) error {
	return r.c.Delete(ctx,
		fmt.Sprintf("/api/principals/%s/client-access/%s", principalID, clientID), nil)
}

// ResetPassword — POST /api/principals/{id}/reset-password.
func (r *PrincipalsResource) ResetPassword(ctx context.Context, principalID string, req *ResetPasswordRequest) error {
	return r.c.Post(ctx, "/api/principals/"+principalID+"/reset-password", req, nil)
}

// Sync — POST /api/applications/{appCode}/principals/sync. When
// removeUnlisted is true the platform strips SDK-sourced role
// assignments from principals not in the list (principals themselves
// are never deleted by sync). A PasswordHash is used only to create a
// user; SyncResult.PasswordHashIgnored lists the existing users whose
// hash was not applied.
func (r *PrincipalsResource) Sync(ctx context.Context, appCode string, req *SyncPrincipalsRequest, removeUnlisted bool) (*SyncResult, error) {
	q := ""
	if removeUnlisted {
		q = "?removeUnlisted=true"
	}
	var out SyncResult
	if err := r.c.Post(ctx, fmt.Sprintf("/api/applications/%s/principals/sync%s", appCode, q), req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}
