package client

import "context"

// ─── Request DTOs ────────────────────────────────────────────────────

// CreateConnectionRequest — POST /api/connections.
type CreateConnectionRequest struct {
	Code             string `json:"code"`
	Name             string `json:"name"`
	Description      string `json:"description,omitempty"`
	ServiceAccountID string `json:"serviceAccountId"`
	ExternalID       string `json:"externalId,omitempty"`
	ClientID         string `json:"clientId,omitempty"`
	ApplicationCode  string `json:"applicationCode,omitempty"`
}

// UpdateConnectionRequest — PUT /api/connections/{id}.
//
// Name is required and always sent: the platform replaces the name on
// every update. Nil optional members are left unchanged.
type UpdateConnectionRequest struct {
	Name            string  `json:"name"`
	Description     *string `json:"description,omitempty"`
	ExternalID      *string `json:"externalId,omitempty"`
	Status          *string `json:"status,omitempty"`
	ApplicationCode *string `json:"applicationCode,omitempty"`
}

// ─── Response DTOs ───────────────────────────────────────────────────

// ConnectionResponse is the platform's connection aggregate.
type ConnectionResponse struct {
	ID               string `json:"id"`
	Code             string `json:"code"`
	Name             string `json:"name"`
	Description      string `json:"description,omitempty"`
	ExternalID       string `json:"externalId,omitempty"`
	Status           string `json:"status"`
	ServiceAccountID string `json:"serviceAccountId"`
	ClientID         string `json:"clientId,omitempty"`
	ClientIdentifier string `json:"clientIdentifier,omitempty"`
	ApplicationCode  string `json:"applicationCode,omitempty"`
	Source           string `json:"source"`
	CreatedAt        string `json:"createdAt"`
	UpdatedAt        string `json:"updatedAt"`
}

// ConnectionsListResponse — GET /api/connections.
type ConnectionsListResponse struct {
	Connections []ConnectionResponse `json:"connections"`
	Total       uint64               `json:"total,omitempty"`
}

// ─── Resource ────────────────────────────────────────────────────────

// ConnectionsResource — /api/connections/*.
type ConnectionsResource struct {
	c *FlowCatalystClient
}

// Create — POST /api/connections. Returns the created connection.
func (r *ConnectionsResource) Create(ctx context.Context, req *CreateConnectionRequest) (*ConnectionResponse, error) {
	var out ConnectionResponse
	if err := r.c.Post(ctx, "/api/connections", req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Get — GET /api/connections/{id}.
func (r *ConnectionsResource) Get(ctx context.Context, id string) (*ConnectionResponse, error) {
	var out ConnectionResponse
	if err := r.c.Get(ctx, "/api/connections/"+id, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// List — GET /api/connections with optional clientId / status filters
// (pass "" to omit one).
func (r *ConnectionsResource) List(ctx context.Context, clientID, status string) (*ConnectionsListResponse, error) {
	q := NewQuery().
		String("clientId", clientID).
		String("status", status).
		Encode()
	var out ConnectionsListResponse
	if err := r.c.Get(ctx, "/api/connections"+q, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Update — PUT /api/connections/{id}. The platform answers 204.
func (r *ConnectionsResource) Update(ctx context.Context, id string, req *UpdateConnectionRequest) error {
	return r.c.Put(ctx, "/api/connections/"+id, req, nil)
}

// Delete — DELETE /api/connections/{id}.
func (r *ConnectionsResource) Delete(ctx context.Context, id string) error {
	return r.c.Delete(ctx, "/api/connections/"+id, nil)
}

// Pause — POST /api/connections/{id}/pause.
func (r *ConnectionsResource) Pause(ctx context.Context, id string) error {
	return r.c.Post(ctx, "/api/connections/"+id+"/pause", nil, nil)
}

// Activate — POST /api/connections/{id}/activate.
func (r *ConnectionsResource) Activate(ctx context.Context, id string) error {
	return r.c.Post(ctx, "/api/connections/"+id+"/activate", nil, nil)
}
