package client

import (
	"context"
	"fmt"
)

// CreateDispatchPoolRequest — POST /api/dispatch-pools.
type CreateDispatchPoolRequest struct {
	Code        string `json:"code"`
	Name        string `json:"name"`
	Description string `json:"description,omitempty"`
	// Concurrency is the max concurrent in-flight dispatches.
	Concurrency uint32 `json:"concurrency,omitempty"`
	// RateLimit is messages per minute; nil means no rate limit.
	RateLimit *uint32 `json:"rateLimit,omitempty"`
	ClientID  string  `json:"clientId,omitempty"`
}

// UpdateDispatchPoolRequest — PUT /api/dispatch-pools/{id}. Nil members
// are left unchanged.
type UpdateDispatchPoolRequest struct {
	Name        *string `json:"name,omitempty"`
	Description *string `json:"description,omitempty"`
	Concurrency *uint32 `json:"concurrency,omitempty"`
	RateLimit   *uint32 `json:"rateLimit,omitempty"`
}

// DispatchPoolResponse is the platform's dispatch pool.
type DispatchPoolResponse struct {
	ID               string  `json:"id"`
	Code             string  `json:"code"`
	Name             string  `json:"name"`
	Description      string  `json:"description,omitempty"`
	Concurrency      uint32  `json:"concurrency"`
	RateLimit        *uint32 `json:"rateLimit,omitempty"`
	Status           string  `json:"status"`
	ClientID         string  `json:"clientId,omitempty"`
	ClientIdentifier string  `json:"clientIdentifier,omitempty"`
	CreatedAt        string  `json:"createdAt"`
	UpdatedAt        string  `json:"updatedAt"`
}

// DispatchPoolListResponse — GET /api/dispatch-pools.
type DispatchPoolListResponse struct {
	Pools []DispatchPoolResponse `json:"pools"`
	Total uint64                 `json:"total"`
}

// SyncDispatchPoolItem — one item in the sync payload.
type SyncDispatchPoolItem struct {
	Code        string  `json:"code"`
	Name        string  `json:"name"`
	Description string  `json:"description,omitempty"`
	Concurrency uint32  `json:"concurrency,omitempty"`
	RateLimit   *uint32 `json:"rateLimit,omitempty"`
}

// SyncDispatchPoolsRequest — body for the per-app sync endpoint.
type SyncDispatchPoolsRequest struct {
	Pools []SyncDispatchPoolItem `json:"pools"`
}

// DispatchPoolsResource — /api/dispatch-pools/*.
type DispatchPoolsResource struct {
	c *FlowCatalystClient
}

// Create — POST /api/dispatch-pools. Returns the new pool's id; call Get
// for the full record.
func (r *DispatchPoolsResource) Create(ctx context.Context, req *CreateDispatchPoolRequest) (*CreatedResponse, error) {
	var out CreatedResponse
	if err := r.c.Post(ctx, "/api/dispatch-pools", req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Get — GET /api/dispatch-pools/{id}.
func (r *DispatchPoolsResource) Get(ctx context.Context, id string) (*DispatchPoolResponse, error) {
	var out DispatchPoolResponse
	if err := r.c.Get(ctx, "/api/dispatch-pools/"+id, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// List — GET /api/dispatch-pools?clientId=&status=.
func (r *DispatchPoolsResource) List(ctx context.Context, clientID, status string) (*DispatchPoolListResponse, error) {
	q := EncodeQuery("clientId", clientID, "status", status)
	var out DispatchPoolListResponse
	if err := r.c.Get(ctx, "/api/dispatch-pools"+q, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Update — PUT /api/dispatch-pools/{id}. The platform answers 204; call
// Get for the refreshed record.
func (r *DispatchPoolsResource) Update(ctx context.Context, id string, req *UpdateDispatchPoolRequest) error {
	return r.c.Put(ctx, "/api/dispatch-pools/"+id, req, nil)
}

// Archive — POST /api/dispatch-pools/{id}/archive (soft archive; the row
// is kept with status ARCHIVED). The platform answers 204.
func (r *DispatchPoolsResource) Archive(ctx context.Context, id string) error {
	return r.c.Post(ctx, "/api/dispatch-pools/"+id+"/archive", nil, nil)
}

// Delete — DELETE /api/dispatch-pools/{id} (hard delete). Most callers
// want Archive instead.
func (r *DispatchPoolsResource) Delete(ctx context.Context, id string) error {
	return r.c.Delete(ctx, "/api/dispatch-pools/"+id, nil)
}

// Suspend — POST /api/dispatch-pools/{id}/suspend. The platform answers 204.
func (r *DispatchPoolsResource) Suspend(ctx context.Context, id string) error {
	return r.c.Post(ctx, "/api/dispatch-pools/"+id+"/suspend", nil, nil)
}

// Activate — POST /api/dispatch-pools/{id}/activate. The platform answers 204.
func (r *DispatchPoolsResource) Activate(ctx context.Context, id string) error {
	return r.c.Post(ctx, "/api/dispatch-pools/"+id+"/activate", nil, nil)
}

// Sync — POST /api/applications/{appCode}/dispatch-pools/sync.
func (r *DispatchPoolsResource) Sync(ctx context.Context, appCode string, req *SyncDispatchPoolsRequest, removeUnlisted bool) (*SyncResult, error) {
	q := ""
	if removeUnlisted {
		q = "?removeUnlisted=true"
	}
	var out SyncResult
	if err := r.c.Post(ctx, fmt.Sprintf("/api/applications/%s/dispatch-pools/sync%s", appCode, q), req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}
