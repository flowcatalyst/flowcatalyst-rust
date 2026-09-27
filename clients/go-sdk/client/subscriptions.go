package client

import (
	"context"
	"fmt"
)

// SubscriptionListResponse — GET /api/subscriptions.
type SubscriptionListResponse struct {
	Subscriptions []SubscriptionResponse `json:"subscriptions"`
	Total         uint64                 `json:"total,omitempty"`
}

// ConfigEntry is a per-subscription custom config key/value.
type ConfigEntry struct {
	Key   string `json:"key"`
	Value string `json:"value"`
}

// EventTypeBinding pairs an event type code/pattern with an optional
// filter expression. Patterns support `*` wildcards per segment.
type EventTypeBinding struct {
	EventTypeCode string `json:"eventTypeCode"`
	Filter        string `json:"filter,omitempty"`
}

// CreateSubscriptionRequest — POST /api/subscriptions.
type CreateSubscriptionRequest struct {
	Code             string             `json:"code"`
	Name             string             `json:"name"`
	Description      string             `json:"description,omitempty"`
	Endpoint         string             `json:"endpoint"`
	ConnectionID     string             `json:"connectionId,omitempty"`
	EventTypes       []EventTypeBinding `json:"eventTypes,omitempty"`
	ClientID         string             `json:"clientId,omitempty"`
	DispatchPoolID   string             `json:"dispatchPoolId,omitempty"`
	ServiceAccountID string             `json:"serviceAccountId,omitempty"`
	Mode             string             `json:"mode,omitempty"`
	TimeoutSeconds   *uint32            `json:"timeoutSeconds,omitempty"`
	MaxRetries       *uint32            `json:"maxRetries,omitempty"`
	DataOnly         bool               `json:"dataOnly,omitempty"`
}

// UpdateSubscriptionRequest — PUT /api/subscriptions/{id}.
type UpdateSubscriptionRequest struct {
	Name           *string `json:"name,omitempty"`
	Description    *string `json:"description,omitempty"`
	Endpoint       *string `json:"endpoint,omitempty"`
	ConnectionID   *string `json:"connectionId,omitempty"`
	TimeoutSeconds *uint32 `json:"timeoutSeconds,omitempty"`
	MaxRetries     *uint32 `json:"maxRetries,omitempty"`
}

// SubscriptionResponse is the platform's subscription representation.
type SubscriptionResponse struct {
	ID               string             `json:"id"`
	Code             string             `json:"code"`
	Name             string             `json:"name"`
	Description      string             `json:"description,omitempty"`
	ClientID         string             `json:"clientId,omitempty"`
	ClientIdentifier string             `json:"clientIdentifier,omitempty"`
	EventTypes       []EventTypeBinding `json:"eventTypes,omitempty"`
	Endpoint         string             `json:"endpoint"`
	ConnectionID     string             `json:"connectionId,omitempty"`
	Queue            string             `json:"queue,omitempty"`
	Source           string             `json:"source,omitempty"`
	Status           string             `json:"status"`
	MaxAgeSeconds    uint32             `json:"maxAgeSeconds,omitempty"`
	DispatchPoolID   string             `json:"dispatchPoolId,omitempty"`
	DispatchPoolCode string             `json:"dispatchPoolCode,omitempty"`
	DelaySeconds     uint32             `json:"delaySeconds,omitempty"`
	Sequence         int32              `json:"sequence,omitempty"`
	Mode             string             `json:"mode"`
	TimeoutSeconds   uint32             `json:"timeoutSeconds,omitempty"`
	MaxRetries       uint32             `json:"maxRetries,omitempty"`
	ServiceAccountID string             `json:"serviceAccountId,omitempty"`
	DataOnly         bool               `json:"dataOnly,omitempty"`
	ApplicationCode  string             `json:"applicationCode,omitempty"`
	ClientScoped     bool               `json:"clientScoped,omitempty"`
	CustomConfig     []ConfigEntry      `json:"customConfig,omitempty"`
	CreatedAt        string             `json:"createdAt"`
	UpdatedAt        string             `json:"updatedAt"`
}

// SyncSubscriptionItem matches the platform's SyncSubscriptionInput.
type SyncSubscriptionItem struct {
	Code             string             `json:"code"`
	Name             string             `json:"name"`
	Description      string             `json:"description,omitempty"`
	Target           string             `json:"target"`
	ConnectionID     string             `json:"connectionId,omitempty"`
	EventTypes       []EventTypeBinding `json:"eventTypes"`
	DispatchPoolCode string             `json:"dispatchPoolCode,omitempty"`
	Mode             string             `json:"mode,omitempty"`
	MaxRetries       *uint32            `json:"maxRetries,omitempty"`
	TimeoutSeconds   *uint32            `json:"timeoutSeconds,omitempty"`
	DataOnly         bool               `json:"dataOnly,omitempty"`
}

// SyncSubscriptionsRequest — body for the per-app sync endpoint.
type SyncSubscriptionsRequest struct {
	Subscriptions []SyncSubscriptionItem `json:"subscriptions"`
}

// SubscriptionsResource — /api/subscriptions/*.
type SubscriptionsResource struct {
	c *FlowCatalystClient
}

// Create — POST /api/subscriptions. Returns the new subscription's id;
// call Get for the full record.
func (r *SubscriptionsResource) Create(ctx context.Context, req *CreateSubscriptionRequest) (*CreatedResponse, error) {
	var out CreatedResponse
	if err := r.c.Post(ctx, "/api/subscriptions", req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Get — GET /api/subscriptions/{id}.
func (r *SubscriptionsResource) Get(ctx context.Context, id string) (*SubscriptionResponse, error) {
	var out SubscriptionResponse
	if err := r.c.Get(ctx, "/api/subscriptions/"+id, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// List — GET /api/subscriptions?clientId=&status=.
func (r *SubscriptionsResource) List(ctx context.Context, clientID, status string) (*SubscriptionListResponse, error) {
	q := EncodeQuery("clientId", clientID, "status", status)
	var out SubscriptionListResponse
	if err := r.c.Get(ctx, "/api/subscriptions"+q, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Update — PUT /api/subscriptions/{id}. The platform answers 204; call
// Get for the refreshed record.
func (r *SubscriptionsResource) Update(ctx context.Context, id string, req *UpdateSubscriptionRequest) error {
	return r.c.Put(ctx, "/api/subscriptions/"+id, req, nil)
}

// Pause — POST /api/subscriptions/{id}/pause.
func (r *SubscriptionsResource) Pause(ctx context.Context, id string) error {
	return r.c.Post(ctx, "/api/subscriptions/"+id+"/pause", nil, nil)
}

// Resume — POST /api/subscriptions/{id}/resume.
func (r *SubscriptionsResource) Resume(ctx context.Context, id string) error {
	return r.c.Post(ctx, "/api/subscriptions/"+id+"/resume", nil, nil)
}

// Delete — DELETE /api/subscriptions/{id}.
func (r *SubscriptionsResource) Delete(ctx context.Context, id string) error {
	return r.c.Delete(ctx, "/api/subscriptions/"+id, nil)
}

// Sync — POST /api/applications/{appCode}/subscriptions/sync.
func (r *SubscriptionsResource) Sync(ctx context.Context, appCode string, req *SyncSubscriptionsRequest, removeUnlisted bool) (*SyncResult, error) {
	q := ""
	if removeUnlisted {
		q = "?removeUnlisted=true"
	}
	var out SyncResult
	path := fmt.Sprintf("/api/applications/%s/subscriptions/sync%s", appCode, q)
	if err := r.c.Post(ctx, path, req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}
