package client

import (
	"context"
	"fmt"
)

// CreateProcessRequest — POST /api/processes.
type CreateProcessRequest struct {
	// Code follows {app}:{subdomain}:{process}.
	Code        string `json:"code"`
	Name        string `json:"name"`
	Description string `json:"description,omitempty"`
	// Body is the diagram source, stored verbatim (typically Mermaid).
	Body string `json:"body,omitempty"`
	// DiagramType is the diagram language; the platform applies "mermaid"
	// when omitted.
	DiagramType string   `json:"diagramType,omitempty"`
	Tags        []string `json:"tags,omitempty"`
}

// UpdateProcessRequest — PUT /api/processes/{id}. Nil members are left
// unchanged; a non-nil Tags (even empty) replaces the tag list.
type UpdateProcessRequest struct {
	Name        *string   `json:"name,omitempty"`
	Description *string   `json:"description,omitempty"`
	Body        *string   `json:"body,omitempty"`
	DiagramType *string   `json:"diagramType,omitempty"`
	Tags        *[]string `json:"tags,omitempty"`
}

// ProcessResponse is the platform's process documentation aggregate.
type ProcessResponse struct {
	ID          string   `json:"id"`
	Code        string   `json:"code"`
	Name        string   `json:"name"`
	Description string   `json:"description,omitempty"`
	Status      string   `json:"status"`
	Source      string   `json:"source"`
	Application string   `json:"application"`
	Subdomain   string   `json:"subdomain"`
	ProcessName string   `json:"processName"`
	Body        string   `json:"body"`
	DiagramType string   `json:"diagramType"`
	Tags        []string `json:"tags"`
	CreatedBy   string   `json:"createdBy,omitempty"`
	CreatedAt   string   `json:"createdAt"`
	UpdatedAt   string   `json:"updatedAt"`
}

// ProcessListResponse — GET /api/processes.
type ProcessListResponse struct {
	Items []ProcessResponse `json:"items"`
}

// SyncProcessInput — one item in the sync payload. The platform rejects
// any member not listed here.
type SyncProcessInput struct {
	Code        string   `json:"code"`
	Name        string   `json:"name"`
	Description string   `json:"description,omitempty"`
	Body        string   `json:"body,omitempty"`
	DiagramType string   `json:"diagramType,omitempty"`
	Tags        []string `json:"tags,omitempty"`
}

// SyncProcessesRequest — body for the per-app sync endpoint.
type SyncProcessesRequest struct {
	Processes []SyncProcessInput `json:"processes"`
}

// ProcessesResource — /api/processes/*.
type ProcessesResource struct {
	c *FlowCatalystClient
}

// Create — POST /api/processes. Returns the new process's id; call Get
// for the full record.
func (r *ProcessesResource) Create(ctx context.Context, req *CreateProcessRequest) (*CreatedResponse, error) {
	var out CreatedResponse
	if err := r.c.Post(ctx, "/api/processes", req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Get — GET /api/processes/{id}.
func (r *ProcessesResource) Get(ctx context.Context, id string) (*ProcessResponse, error) {
	var out ProcessResponse
	if err := r.c.Get(ctx, "/api/processes/"+id, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// GetByCode — GET /api/processes/by-code/{code}.
func (r *ProcessesResource) GetByCode(ctx context.Context, code string) (*ProcessResponse, error) {
	var out ProcessResponse
	if err := r.c.Get(ctx, "/api/processes/by-code/"+code, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// ProcessFilters — query parameters for GET /api/processes. Empty
// members are omitted.
type ProcessFilters struct {
	Application string
	Subdomain   string
	Status      string
}

// List — GET /api/processes with optional filters (nil for none).
func (r *ProcessesResource) List(ctx context.Context, filters *ProcessFilters) (*ProcessListResponse, error) {
	q := ""
	if filters != nil {
		q = EncodeQuery("application", filters.Application, "subdomain", filters.Subdomain, "status", filters.Status)
	}
	var out ProcessListResponse
	if err := r.c.Get(ctx, "/api/processes"+q, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Update — PUT /api/processes/{id}. The platform answers 204; call Get
// for the refreshed record.
func (r *ProcessesResource) Update(ctx context.Context, id string, req *UpdateProcessRequest) error {
	return r.c.Put(ctx, "/api/processes/"+id, req, nil)
}

// Archive — POST /api/processes/{id}/archive (soft archive; the row is
// kept with status ARCHIVED). The platform answers 204.
func (r *ProcessesResource) Archive(ctx context.Context, id string) error {
	return r.c.Post(ctx, "/api/processes/"+id+"/archive", nil, nil)
}

// Delete — DELETE /api/processes/{id} (hard delete). Most callers want
// Archive instead.
func (r *ProcessesResource) Delete(ctx context.Context, id string) error {
	return r.c.Delete(ctx, "/api/processes/"+id, nil)
}

// Sync — POST /api/applications/{appCode}/processes/sync.
func (r *ProcessesResource) Sync(ctx context.Context, appCode string, req *SyncProcessesRequest, removeUnlisted bool) (*SyncResult, error) {
	q := ""
	if removeUnlisted {
		q = "?removeUnlisted=true"
	}
	var out SyncResult
	if err := r.c.Post(ctx, fmt.Sprintf("/api/applications/%s/processes/sync%s", appCode, q), req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}
