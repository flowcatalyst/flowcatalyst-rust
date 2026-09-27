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
	// ClientScoped marks events of this type as carried per client.
	ClientScoped bool `json:"clientScoped,omitempty"`
}

// UpdateEventTypeRequest is the body for PUT /api/event-types/{id}.
//
// Name is required and always sent: the platform replaces the name on
// every update. Nil optional members are left unchanged.
type UpdateEventTypeRequest struct {
	Name         string  `json:"name"`
	Description  *string `json:"description,omitempty"`
	ClientScoped *bool   `json:"clientScoped,omitempty"`
}

// AddSchemaVersionRequest is the body for POST /api/event-types/{id}/versions.
type AddSchemaVersionRequest struct {
	// Version is the spec version label (e.g. "1.1"). The Go platform
	// requires it (always sent); a platform that assigns versions itself
	// ignores it.
	Version string          `json:"version"`
	Schema  json.RawMessage `json:"schema"`
}

// EventTypeResponse is the platform's event-type representation.
type EventTypeResponse struct {
	ID           string                `json:"id"`
	Code         string                `json:"code"`
	Name         string                `json:"name"`
	Description  string                `json:"description,omitempty"`
	Status       string                `json:"status"`
	Source       string                `json:"source"`
	Application  string                `json:"application"`
	Subdomain    string                `json:"subdomain"`
	Aggregate    string                `json:"aggregate"`
	EventName    string                `json:"eventName"`
	ClientID     string                `json:"clientId,omitempty"`
	CreatedBy    string                `json:"createdBy,omitempty"`
	SpecVersions []SpecVersionResponse `json:"specVersions"`
	CreatedAt    string                `json:"createdAt"`
	UpdatedAt    string                `json:"updatedAt"`
}

// SpecVersionResponse is one schema version on an event type.
type SpecVersionResponse struct {
	Version   string          `json:"version"`
	Status    string          `json:"status"`
	Schema    json.RawMessage `json:"schema,omitempty"`
	CreatedAt string          `json:"createdAt,omitempty"`
}

// SyncEventTypeItem is one event type in the per-app sync payload. The
// platform rejects any other member (schemas are added separately via
// AddSchemaVersion; sync is application-scoped, so there is no clientId).
type SyncEventTypeItem struct {
	Code        string `json:"code"`
	Name        string `json:"name"`
	Description string `json:"description,omitempty"`
}

// SyncEventTypesRequest is the body for the per-app sync endpoint.
type SyncEventTypesRequest struct {
	EventTypes []SyncEventTypeItem `json:"eventTypes"`
}

// EventTypesResource is the accessor for /api/event-types/*.
// Construct via FlowCatalystClient.EventTypes.
type EventTypesResource struct {
	c *FlowCatalystClient
}

// Create — POST /api/event-types. Returns the new event type's id; call
// Get for the full record.
func (r *EventTypesResource) Create(ctx context.Context, req *CreateEventTypeRequest) (*CreatedResponse, error) {
	var out CreatedResponse
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

// List — GET /api/event-types?application=&status=&clientId=. With no
// filter at all the platform returns only CURRENT event types.
func (r *EventTypesResource) List(ctx context.Context, application, status, clientID string) (*EventTypeListResponse, error) {
	q := EncodeQuery("application", application, "status", status, "clientId", clientID)
	var out EventTypeListResponse
	if err := r.c.Get(ctx, "/api/event-types"+q, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Update — PUT /api/event-types/{id}. The platform answers 204; call Get
// for the refreshed record.
func (r *EventTypesResource) Update(ctx context.Context, id string, req *UpdateEventTypeRequest) error {
	return r.c.Put(ctx, "/api/event-types/"+id, req, nil)
}

// AddSchemaVersion — POST /api/event-types/{id}/versions. Returns the
// updated event type.
func (r *EventTypesResource) AddSchemaVersion(ctx context.Context, id string, req *AddSchemaVersionRequest) (*EventTypeResponse, error) {
	var out EventTypeResponse
	if err := r.c.Post(ctx, "/api/event-types/"+id+"/versions", req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Delete — DELETE /api/event-types/{id}. This is a hard delete: the
// platform removes the row (emitting EventTypeDeleted). There is no
// archive route for event types; to retire event types an application
// owns, drop them from its definitions and Sync with removeUnlisted.
func (r *EventTypesResource) Delete(ctx context.Context, id string) error {
	return r.c.Delete(ctx, "/api/event-types/"+id, nil)
}

// Sync — POST /api/applications/{appCode}/event-types/sync. Declarative
// reconciliation: the request's eventTypes list becomes the desired
// state. With removeUnlisted=true the server removes any existing
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
