package client

import (
	"context"
	"encoding/json"
	"fmt"
)

// EventTypeListResponse — GET /api/event-types returns `{ items: [...] }`.
type EventTypeListResponse struct {
	Items []EventTypeResponse `json:"items"`
}

// CreateEventTypeRequest is the body for POST /api/event-types.
type CreateEventTypeRequest struct {
	// Code follows {app}:{domain}:{aggregate}:{event}.
	Code string `json:"code"`
	// Human-readable name.
	Name string `json:"name"`
	// Optional description.
	Description string `json:"description,omitempty"`
	// Optional initial JSON schema. Use json.RawMessage to keep the
	// shape opaque end-to-end.
	Schema json.RawMessage `json:"schema,omitempty"`
	// Client ID for multi-tenant scoping.
	ClientID string `json:"clientId,omitempty"`
}

// UpdateEventTypeRequest is the body for PUT /api/event-types/{id}.
type UpdateEventTypeRequest struct {
	Name        *string `json:"name,omitempty"`
	Description *string `json:"description,omitempty"`
}

// AddSchemaVersionRequest is the body for POST /api/event-types/{id}/versions.
type AddSchemaVersionRequest struct {
	// Version is the spec version label (e.g. "1.1"). The Go platform
	// requires it; a platform that assigns versions itself ignores it.
	Version string          `json:"version,omitempty"`
	Schema  json.RawMessage `json:"schema"`
}

// EventTypeResponse is the platform's event-type representation.
type EventTypeResponse struct {
	ID            string                `json:"id"`
	Code          string                `json:"code"`
	Name          string                `json:"name"`
	Description   string                `json:"description,omitempty"`
	Status        string                `json:"status"`
	Application   string                `json:"application,omitempty"`
	Subdomain     string                `json:"subdomain,omitempty"`
	Aggregate     string                `json:"aggregate,omitempty"`
	EventName     string                `json:"event,omitempty"`
	SpecVersions  []SpecVersionResponse `json:"specVersions,omitempty"`
	CreatedAt     string                `json:"createdAt"`
	UpdatedAt     string                `json:"updatedAt"`
}

// SpecVersionResponse is one schema version on an event type.
type SpecVersionResponse struct {
	Version string          `json:"version"`
	Status  string          `json:"status"`
	Schema  json.RawMessage `json:"schema,omitempty"`
}

// SyncEventTypesRequest is the body for the per-app sync endpoint.
type SyncEventTypesRequest struct {
	EventTypes []CreateEventTypeRequest `json:"eventTypes"`
}

// EventTypesResource is the accessor for /api/event-types/*.
// Construct via FlowCatalystClient.EventTypes.
type EventTypesResource struct {
	c *FlowCatalystClient
}

// Create — POST /api/event-types.
func (r *EventTypesResource) Create(ctx context.Context, req *CreateEventTypeRequest) (*EventTypeResponse, error) {
	var out EventTypeResponse
	if err := r.c.Post(ctx, "/api/event-types", req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Get — GET /api/event-types/{id}.
func (r *EventTypesResource) Get(ctx context.Context, id string) (*EventTypeResponse, error) {
	var out EventTypeResponse
	if err := r.c.Get(ctx, "/api/event-types/"+id, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// GetByCode — GET /api/event-types/by-code/{code}.
func (r *EventTypesResource) GetByCode(ctx context.Context, code string) (*EventTypeResponse, error) {
	var out EventTypeResponse
	if err := r.c.Get(ctx, "/api/event-types/by-code/"+code, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// List — GET /api/event-types?application=&status=&clientId=.
func (r *EventTypesResource) List(ctx context.Context, application, status, clientID string) (*EventTypeListResponse, error) {
	q := EncodeQuery("application", application, "status", status, "clientId", clientID)
	var out EventTypeListResponse
	if err := r.c.Get(ctx, "/api/event-types"+q, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Update — PUT /api/event-types/{id}.
func (r *EventTypesResource) Update(ctx context.Context, id string, req *UpdateEventTypeRequest) (*EventTypeResponse, error) {
	var out EventTypeResponse
	if err := r.c.Put(ctx, "/api/event-types/"+id, req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// AddSchemaVersion — POST /api/event-types/{id}/versions.
func (r *EventTypesResource) AddSchemaVersion(ctx context.Context, id string, req *AddSchemaVersionRequest) (*EventTypeResponse, error) {
	var out EventTypeResponse
	if err := r.c.Post(ctx, "/api/event-types/"+id+"/versions", req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Archive — DELETE /api/event-types/{id}. The platform soft-archives:
// the row is retained with status flipped to ARCHIVED. Named Archive
// (not Delete) to make that explicit.
func (r *EventTypesResource) Archive(ctx context.Context, id string) error {
	return r.c.Delete(ctx, "/api/event-types/"+id, nil)
}

// Sync — POST /api/applications/{appCode}/event-types/sync. Declarative
// reconciliation: the request's eventTypes list becomes the desired
// state. With removeUnlisted=true the server archives any existing
// event types not in the request.
func (r *EventTypesResource) Sync(ctx context.Context, appCode string, req *SyncEventTypesRequest, removeUnlisted bool) (*SyncResult, error) {
	q := ""
	if removeUnlisted {
		q = "?removeUnlisted=true"
	}
	var out SyncResult
	path := fmt.Sprintf("/api/applications/%s/event-types/sync%s", appCode, q)
	if err := r.c.Post(ctx, path, req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}
