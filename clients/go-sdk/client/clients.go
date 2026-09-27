package client

import (
	"context"
	"fmt"
)

// ─── Request DTOs ────────────────────────────────────────────────────

// CreateClientRequest — POST /api/clients.
type CreateClientRequest struct {
	Identifier string `json:"identifier"`
	Name       string `json:"name"`
}

// UpdateClientRequest — PUT /api/clients/{id}.
type UpdateClientRequest struct {
	Name *string `json:"name,omitempty"`
}

// StatusChangeRequest — body for suspend / deactivate.
type StatusChangeRequest struct {
	Reason string `json:"reason"`
}

// AddNoteRequest — body for POST /api/clients/{id}/notes.
type AddNoteRequest struct {
	Category string `json:"category"`
	Text     string `json:"text"`
}

// UpdateClientApplicationsRequest — bulk enable/disable.
type UpdateClientApplicationsRequest struct {
	EnabledApplicationIDs []string `json:"enabledApplicationIds"`
}

// ─── Response DTOs ───────────────────────────────────────────────────

// ClientResponse is the platform's client (tenant) aggregate.
type ClientResponse struct {
	ID              string         `json:"id"`
	Name            string         `json:"name"`
	Identifier      string         `json:"identifier"`
	Status          string         `json:"status"`
	StatusReason    string         `json:"statusReason,omitempty"`
	StatusChangedAt string         `json:"statusChangedAt,omitempty"`
	Notes           []NoteResponse `json:"notes"`
	CreatedAt       string         `json:"createdAt"`
	UpdatedAt       string         `json:"updatedAt"`
}

// NoteResponse is one note on a client.
type NoteResponse struct {
	Category string `json:"category"`
	Text     string `json:"text"`
	AddedBy  string `json:"addedBy,omitempty"`
	AddedAt  string `json:"addedAt"`
}

// ClientListResponse — GET /api/clients.
type ClientListResponse struct {
	Clients []ClientResponse `json:"clients"`
	Total   uint64           `json:"total,omitempty"`
}

// StatusChangeResponse — { message } envelope from status-change endpoints.
type StatusChangeResponse struct {
	Message string `json:"message"`
}

// AddNoteResponse — { message } envelope from add-note endpoint.
type AddNoteResponse struct {
	Message string `json:"message"`
}

// ClientApplicationResponse — one entry in GET /api/clients/{id}/applications.
type ClientApplicationResponse struct {
	ID               string `json:"id"`
	Code             string `json:"code"`
	Name             string `json:"name"`
	Description      string `json:"description,omitempty"`
	IconURL          string `json:"iconUrl,omitempty"`
	Active           bool   `json:"active,omitempty"`
	EnabledForClient bool   `json:"enabledForClient,omitempty"`
}

// ClientApplicationsResponse — GET /api/clients/{id}/applications.
type ClientApplicationsResponse struct {
	Applications []ClientApplicationResponse `json:"applications"`
	Total        uint64                      `json:"total,omitempty"`
}

// ─── Resource ────────────────────────────────────────────────────────

// ClientsResource — /api/clients/*.
type ClientsResource struct {
	c *FlowCatalystClient
}

// Create — POST /api/clients.
func (r *ClientsResource) Create(ctx context.Context, req *CreateClientRequest) (*CreatedResponse, error) {
	var out CreatedResponse
	if err := r.c.Post(ctx, "/api/clients", req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// List — GET /api/clients. The platform takes no filters and does not
// page this list; use Search to narrow it.
func (r *ClientsResource) List(ctx context.Context) (*ClientListResponse, error) {
	var out ClientListResponse
	if err := r.c.Get(ctx, "/api/clients", &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Get — GET /api/clients/{id}.
func (r *ClientsResource) Get(ctx context.Context, id string) (*ClientResponse, error) {
	var out ClientResponse
	if err := r.c.Get(ctx, "/api/clients/"+id, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// GetByIdentifier — GET /api/clients/by-identifier/{identifier}.
func (r *ClientsResource) GetByIdentifier(ctx context.Context, identifier string) (*ClientResponse, error) {
	var out ClientResponse
	if err := r.c.Get(ctx, "/api/clients/by-identifier/"+identifier, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Search — GET /api/clients/search?q=...
func (r *ClientsResource) Search(ctx context.Context, query string) (*ClientListResponse, error) {
	q := NewQuery().String("q", query).Encode()
	var out ClientListResponse
	if err := r.c.Get(ctx, "/api/clients/search"+q, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Update — PUT /api/clients/{id}. The platform answers 204; call Get for
// the refreshed record.
func (r *ClientsResource) Update(ctx context.Context, id string, req *UpdateClientRequest) error {
	return r.c.Put(ctx, "/api/clients/"+id, req, nil)
}

// Delete — DELETE /api/clients/{id}.
func (r *ClientsResource) Delete(ctx context.Context, id string) error {
	return r.c.Delete(ctx, "/api/clients/"+id, nil)
}

// Activate — POST /api/clients/{id}/activate.
func (r *ClientsResource) Activate(ctx context.Context, id string) (*StatusChangeResponse, error) {
	var out StatusChangeResponse
	if err := r.c.Post(ctx, "/api/clients/"+id+"/activate", nil, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Suspend — POST /api/clients/{id}/suspend.
func (r *ClientsResource) Suspend(ctx context.Context, id string, req *StatusChangeRequest) (*StatusChangeResponse, error) {
	var out StatusChangeResponse
	if err := r.c.Post(ctx, "/api/clients/"+id+"/suspend", req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Deactivate — POST /api/clients/{id}/deactivate.
func (r *ClientsResource) Deactivate(ctx context.Context, id string, req *StatusChangeRequest) (*StatusChangeResponse, error) {
	var out StatusChangeResponse
	if err := r.c.Post(ctx, "/api/clients/"+id+"/deactivate", req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// AddNote — POST /api/clients/{id}/notes.
func (r *ClientsResource) AddNote(ctx context.Context, id string, req *AddNoteRequest) (*AddNoteResponse, error) {
	var out AddNoteResponse
	if err := r.c.Post(ctx, "/api/clients/"+id+"/notes", req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// ListApplications — GET /api/clients/{id}/applications (with enabled status).
func (r *ClientsResource) ListApplications(ctx context.Context, clientID string) (*ClientApplicationsResponse, error) {
	var out ClientApplicationsResponse
	if err := r.c.Get(ctx, "/api/clients/"+clientID+"/applications", &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// EnableApplication — POST /api/clients/{clientId}/applications/{applicationId}/enable.
// The platform answers 204.
func (r *ClientsResource) EnableApplication(ctx context.Context, clientID, applicationID string) error {
	return r.c.Post(ctx, fmt.Sprintf("/api/clients/%s/applications/%s/enable", clientID, applicationID), nil, nil)
}

// DisableApplication — POST /api/clients/{clientId}/applications/{applicationId}/disable.
// The platform answers 204.
func (r *ClientsResource) DisableApplication(ctx context.Context, clientID, applicationID string) error {
	return r.c.Post(ctx, fmt.Sprintf("/api/clients/%s/applications/%s/disable", clientID, applicationID), nil, nil)
}

// UpdateApplications — PUT /api/clients/{id}/applications (bulk enable
// list). The platform answers 204.
func (r *ClientsResource) UpdateApplications(ctx context.Context, clientID string, req *UpdateClientApplicationsRequest) error {
	return r.c.Put(ctx, "/api/clients/"+clientID+"/applications", req, nil)
}
