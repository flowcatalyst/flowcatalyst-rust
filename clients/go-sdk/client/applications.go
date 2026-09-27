package client

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
)

// ─── Request DTOs ────────────────────────────────────────────────────

// CreateApplicationRequest — POST /api/applications.
type CreateApplicationRequest struct {
	Code            string `json:"code"`
	Name            string `json:"name"`
	Description     string `json:"description,omitempty"`
	ApplicationType string `json:"type,omitempty"`
	DefaultBaseURL  string `json:"defaultBaseUrl,omitempty"`
	IconURL         string `json:"iconUrl,omitempty"`
	Website         string `json:"website,omitempty"`
	Logo            string `json:"logo,omitempty"`
	LogoMimeType    string `json:"logoMimeType,omitempty"`
}

// UpdateApplicationRequest — PUT /api/applications/{id}. Nil members are
// left unchanged.
type UpdateApplicationRequest struct {
	Name           *string `json:"name,omitempty"`
	Description    *string `json:"description,omitempty"`
	DefaultBaseURL *string `json:"defaultBaseUrl,omitempty"`
	IconURL        *string `json:"iconUrl,omitempty"`
	Website        *string `json:"website,omitempty"`
	Logo           *string `json:"logo,omitempty"`
	LogoMimeType   *string `json:"logoMimeType,omitempty"`
}

// ClientConfigRequest — body for the deprecated
// PUT /api/applications/{id}/clients/{clientId} (see UpdateClientConfig).
type ClientConfigRequest struct {
	Enabled         *bool           `json:"enabled,omitempty"`
	BaseURLOverride *string         `json:"baseUrlOverride,omitempty"`
	Config          json.RawMessage `json:"config,omitempty"`
}

// AttachServiceAccountRequest — body for POST /api/applications/{id}/service-account.
type AttachServiceAccountRequest struct {
	ServiceAccountID   string `json:"serviceAccountId"`
	ServiceAccountCode string `json:"serviceAccountCode"`
}

// ─── Response DTOs ───────────────────────────────────────────────────

// ApplicationResponse is the platform's application aggregate.
type ApplicationResponse struct {
	ID               string `json:"id"`
	Code             string `json:"code"`
	Name             string `json:"name"`
	Description      string `json:"description,omitempty"`
	ApplicationType  string `json:"type"`
	DefaultBaseURL   string `json:"defaultBaseUrl,omitempty"`
	IconURL          string `json:"iconUrl,omitempty"`
	Website          string `json:"website,omitempty"`
	Logo             string `json:"logo,omitempty"`
	LogoMimeType     string `json:"logoMimeType,omitempty"`
	ServiceAccountID string `json:"serviceAccountId,omitempty"`
	HasLoginClient   bool   `json:"hasLoginClient"`
	Active           bool   `json:"active"`
	CreatedAt        string `json:"createdAt"`
	UpdatedAt        string `json:"updatedAt"`
}

// ApplicationListResponse — GET /api/applications.
type ApplicationListResponse struct {
	Applications []ApplicationResponse `json:"applications"`
	Total        uint64                `json:"total,omitempty"`
}

// ServiceAccountResponse is the platform's service account, as
// GET /api/service-accounts/{id} returns it. Alias of ServiceAccount.
type ServiceAccountResponse = ServiceAccount

// ApplicationOAuthClientCredentials — the OAuth client provisioned for an
// application's service account. ClientSecret is shown only once.
type ApplicationOAuthClientCredentials struct {
	ID           string `json:"id"`
	ClientID     string `json:"clientId"`
	ClientSecret string `json:"clientSecret,omitempty"`
}

// ApplicationServiceAccountCredentials — the service account provisioned
// for an application, with its OAuth client credentials.
type ApplicationServiceAccountCredentials struct {
	PrincipalID string                            `json:"principalId"`
	Name        string                            `json:"name"`
	OAuthClient ApplicationOAuthClientCredentials `json:"oauthClient"`
}

// ApplicationProvisionServiceAccountResponse —
// POST /api/applications/{id}/provision-service-account. The OAuth client
// secret in ServiceAccount.OAuthClient.ClientSecret is never shown again.
type ApplicationProvisionServiceAccountResponse struct {
	Message        string                               `json:"message"`
	ServiceAccount ApplicationServiceAccountCredentials `json:"serviceAccount"`
}

// ApplicationRolesResponse — GET /api/applications/by-id/{id}/roles: the
// names of the application's roles.
type ApplicationRolesResponse struct {
	Roles []string `json:"roles"`
}

// ClientConfigResponse — an application's per-client configuration, as
// GET /api/applications/{id}/clients[/{clientId}] returns it.
type ClientConfigResponse struct {
	ID              string          `json:"id"`
	ApplicationID   string          `json:"applicationId"`
	ClientID        string          `json:"clientId"`
	Enabled         bool            `json:"enabled"`
	BaseURLOverride string          `json:"baseUrlOverride,omitempty"`
	ConfigJSON      json.RawMessage `json:"configJson,omitempty"`
	CreatedAt       string          `json:"createdAt"`
	UpdatedAt       string          `json:"updatedAt"`
}

// ClientConfigListResponse — GET /api/applications/{id}/clients.
type ClientConfigListResponse struct {
	Items []ClientConfigResponse `json:"items"`
}

// CreatedResponse — returned by create endpoints that emit only an id.
type CreatedResponse struct {
	ID      string `json:"id"`
	Message string `json:"message,omitempty"`
}

// SuccessResponse — generic { message } envelope.
type SuccessResponse struct {
	Message string `json:"message,omitempty"`
}

// ─── Resource ────────────────────────────────────────────────────────

// ApplicationsResource — /api/applications/*.
type ApplicationsResource struct {
	c *FlowCatalystClient
}

// Create — POST /api/applications. Returns the new application's id.
func (r *ApplicationsResource) Create(ctx context.Context, req *CreateApplicationRequest) (*CreatedResponse, error) {
	var out CreatedResponse
	if err := r.c.Post(ctx, "/api/applications", req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// List — GET /api/applications with optional active and type filters.
// Pass nil / "" for a filter you want omitted. The platform does not page
// this list.
func (r *ApplicationsResource) List(ctx context.Context, active *bool, applicationType string) (*ApplicationListResponse, error) {
	q := NewQuery().Bool("active", active).String("type", applicationType).Encode()
	var out ApplicationListResponse
	if err := r.c.Get(ctx, "/api/applications"+q, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Get — GET /api/applications/{id}.
func (r *ApplicationsResource) Get(ctx context.Context, id string) (*ApplicationResponse, error) {
	var out ApplicationResponse
	if err := r.c.Get(ctx, "/api/applications/"+id, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// GetByCode — GET /api/applications/by-code/{code}.
func (r *ApplicationsResource) GetByCode(ctx context.Context, code string) (*ApplicationResponse, error) {
	var out ApplicationResponse
	if err := r.c.Get(ctx, "/api/applications/by-code/"+code, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Update — PUT /api/applications/{id}. The platform answers 204; call Get
// for the refreshed record.
func (r *ApplicationsResource) Update(ctx context.Context, id string, req *UpdateApplicationRequest) error {
	return r.c.Put(ctx, "/api/applications/"+id, req, nil)
}

// Delete — DELETE /api/applications/{id}.
func (r *ApplicationsResource) Delete(ctx context.Context, id string) error {
	return r.c.Delete(ctx, "/api/applications/"+id, nil)
}

// Activate — POST /api/applications/{id}/activate.
func (r *ApplicationsResource) Activate(ctx context.Context, id string) (*ApplicationResponse, error) {
	var out ApplicationResponse
	if err := r.c.Post(ctx, "/api/applications/"+id+"/activate", nil, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Deactivate — POST /api/applications/{id}/deactivate.
func (r *ApplicationsResource) Deactivate(ctx context.Context, id string) (*ApplicationResponse, error) {
	var out ApplicationResponse
	if err := r.c.Post(ctx, "/api/applications/"+id+"/deactivate", nil, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// ProvisionServiceAccount — POST /api/applications/{id}/provision-service-account.
// The response carries the OAuth client secret
// (ServiceAccount.OAuthClient.ClientSecret), which is never shown again.
func (r *ApplicationsResource) ProvisionServiceAccount(ctx context.Context, id string) (*ApplicationProvisionServiceAccountResponse, error) {
	var out ApplicationProvisionServiceAccountResponse
	if err := r.c.Post(ctx, "/api/applications/"+id+"/provision-service-account", nil, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// AttachServiceAccount — POST /api/applications/{id}/service-account.
// Links an existing service account to the application.
func (r *ApplicationsResource) AttachServiceAccount(ctx context.Context, id string, req *AttachServiceAccountRequest) error {
	return r.c.Post(ctx, "/api/applications/"+id+"/service-account", req, nil)
}

// GetServiceAccount returns the application's service account. The
// platform has no single route for this: it reads the application
// (GET /api/applications/{id}) and then its service account
// (GET /api/service-accounts/{serviceAccountId}). When the application
// has no service account it returns an *APIError with status 404
// (IsNotFound) and code SERVICE_ACCOUNT_NOT_FOUND.
func (r *ApplicationsResource) GetServiceAccount(ctx context.Context, id string) (*ServiceAccountResponse, error) {
	app, err := r.Get(ctx, id)
	if err != nil {
		return nil, err
	}
	if app.ServiceAccountID == "" {
		body, _ := json.Marshal(map[string]string{
			"error":   "SERVICE_ACCOUNT_NOT_FOUND",
			"message": fmt.Sprintf("application %s has no service account", id),
		})
		return nil, &APIError{StatusCode: http.StatusNotFound, Body: string(body)}
	}
	return r.c.ServiceAccounts().Get(ctx, app.ServiceAccountID)
}

// ListRoles — GET /api/applications/by-id/{id}/roles. Returns the names of
// the application's roles.
//
// The platform mounts the admin TSID lookup under /by-id so it doesn't
// collide with the SDK's /{appCode}/roles/sync route.
func (r *ApplicationsResource) ListRoles(ctx context.Context, id string) ([]string, error) {
	var out ApplicationRolesResponse
	if err := r.c.Get(ctx, "/api/applications/by-id/"+id+"/roles", &out); err != nil {
		return nil, err
	}
	return out.Roles, nil
}

// ListClients — GET /api/applications/{id}/clients.
func (r *ApplicationsResource) ListClients(ctx context.Context, id string) (*ClientConfigListResponse, error) {
	var out ClientConfigListResponse
	if err := r.c.Get(ctx, "/api/applications/"+id+"/clients", &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// GetClientConfig — GET /api/applications/{id}/clients/{clientId}.
func (r *ApplicationsResource) GetClientConfig(ctx context.Context, id, clientID string) (*ClientConfigResponse, error) {
	var out ClientConfigResponse
	if err := r.c.Get(ctx, fmt.Sprintf("/api/applications/%s/clients/%s", id, clientID), &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// UpdateClientConfig — PUT /api/applications/{id}/clients/{clientId}.
//
// Deprecated: only the Rust platform serves this PUT; the Go platform has
// no such route and answers 405/404. Use EnableForClient /
// DisableForClient to toggle an application for a client, and
// GetClientConfig to read the configuration.
func (r *ApplicationsResource) UpdateClientConfig(ctx context.Context, id, clientID string, req *ClientConfigRequest) (*ClientConfigResponse, error) {
	var out ClientConfigResponse
	if err := r.c.Put(ctx, fmt.Sprintf("/api/applications/%s/clients/%s", id, clientID), req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// EnableForClient — POST /api/applications/{id}/clients/{clientId}/enable.
// The platform answers 204; call GetClientConfig for the result.
func (r *ApplicationsResource) EnableForClient(ctx context.Context, id, clientID string) error {
	return r.c.Post(ctx, fmt.Sprintf("/api/applications/%s/clients/%s/enable", id, clientID), nil, nil)
}

// DisableForClient — POST /api/applications/{id}/clients/{clientId}/disable.
// The platform answers 204.
func (r *ApplicationsResource) DisableForClient(ctx context.Context, id, clientID string) error {
	return r.c.Post(ctx, fmt.Sprintf("/api/applications/%s/clients/%s/disable", id, clientID), nil, nil)
}
