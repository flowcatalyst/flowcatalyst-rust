package client

import (
	"context"
	"encoding/json"
	"fmt"
)

// ─── Request DTOs ────────────────────────────────────────────────────

// CreateScheduledJobRequest — POST /api/scheduled-jobs.
// Concurrent and TracksCompletion are required by the platform, so they are
// always sent (false included).
type CreateScheduledJobRequest struct {
	Code        string `json:"code"`
	Name        string `json:"name"`
	Description string `json:"description,omitempty"`
	// ClientID empty = platform-scoped (anchor only); set = client-scoped.
	ClientID            string          `json:"clientId,omitempty"`
	Crons               []string        `json:"crons"`
	Timezone            string          `json:"timezone,omitempty"`
	Payload             json.RawMessage `json:"payload,omitempty"`
	Concurrent          bool            `json:"concurrent"`
	TracksCompletion    bool            `json:"tracksCompletion"`
	TimeoutSeconds      *int32          `json:"timeoutSeconds,omitempty"`
	DeliveryMaxAttempts *int32          `json:"deliveryMaxAttempts,omitempty"`
	TargetURL           string          `json:"targetUrl,omitempty"`
}

// UpdateScheduledJobRequest — PUT /api/scheduled-jobs/{id}. All optional.
type UpdateScheduledJobRequest struct {
	Name                *string         `json:"name,omitempty"`
	Description         *string         `json:"description,omitempty"`
	Crons               []string        `json:"crons,omitempty"`
	Timezone            *string         `json:"timezone,omitempty"`
	Payload             json.RawMessage `json:"payload,omitempty"`
	Concurrent          *bool           `json:"concurrent,omitempty"`
	TracksCompletion    *bool           `json:"tracksCompletion,omitempty"`
	TimeoutSeconds      *int32          `json:"timeoutSeconds,omitempty"`
	DeliveryMaxAttempts *int32          `json:"deliveryMaxAttempts,omitempty"`
	TargetURL           *string         `json:"targetUrl,omitempty"`
}

// FireRequest — body for POST /api/scheduled-jobs/{id}/fire.
type FireRequest struct {
	CorrelationID string `json:"correlationId,omitempty"`
}

// LogLevel for instance log entries. Use the Log* constants.
type LogLevel string

const (
	LogLevelDebug LogLevel = "DEBUG"
	LogLevelInfo  LogLevel = "INFO"
	LogLevelWarn  LogLevel = "WARN"
	LogLevelError LogLevel = "ERROR"
)

// InstanceLogRequest — append a log entry to a running instance.
type InstanceLogRequest struct {
	Message  string          `json:"message"`
	Level    LogLevel        `json:"level,omitempty"`
	Metadata json.RawMessage `json:"metadata,omitempty"`
}

// CompletionStatus marks a finished instance.
type CompletionStatus string

const (
	CompletionStatusSuccess CompletionStatus = "SUCCESS"
	CompletionStatusFailure CompletionStatus = "FAILURE"
)

// InstanceCompleteRequest — mark a running instance complete.
type InstanceCompleteRequest struct {
	Status CompletionStatus `json:"status"`
	Result json.RawMessage  `json:"result,omitempty"`
}

// ─── Filters ─────────────────────────────────────────────────────────

// ScheduledJobFilters — query parameters for GET /api/scheduled-jobs.
// Pass ClientID = "platform" to filter platform-scoped only.
type ScheduledJobFilters struct {
	ClientID string
	Status   string
	Search   string
	Page     *uint32
	Size     *uint32
}

// InstanceFilters — query parameters for instance listings.
type InstanceFilters struct {
	Status      string
	TriggerKind string
	From        string
	To          string
	Page        *uint32
	Size        *uint32
}

// ─── Response DTOs ───────────────────────────────────────────────────

// ScheduledJobResponse is the platform's scheduled job aggregate.
type ScheduledJobResponse struct {
	ID                  string          `json:"id"`
	ClientID            string          `json:"clientId,omitempty"`
	Code                string          `json:"code"`
	Name                string          `json:"name"`
	Description         string          `json:"description,omitempty"`
	Status              string          `json:"status"`
	Crons               []string        `json:"crons"`
	Timezone            string          `json:"timezone"`
	Payload             json.RawMessage `json:"payload,omitempty"`
	Concurrent          bool            `json:"concurrent"`
	TracksCompletion    bool            `json:"tracksCompletion"`
	TimeoutSeconds      *int32          `json:"timeoutSeconds,omitempty"`
	DeliveryMaxAttempts int32           `json:"deliveryMaxAttempts"`
	TargetURL           string          `json:"targetUrl,omitempty"`
	LastFiredAt         string          `json:"lastFiredAt,omitempty"`
	CreatedAt           string          `json:"createdAt"`
	UpdatedAt           string          `json:"updatedAt"`
	CreatedBy           string          `json:"createdBy,omitempty"`
	UpdatedBy           string          `json:"updatedBy,omitempty"`
	Version             int32           `json:"version"`
	// HasActiveInstance: computed; true if any non-terminal instance exists.
	HasActiveInstance bool `json:"hasActiveInstance,omitempty"`
}

// ScheduledJobInstanceResponse — one firing of a scheduled job.
type ScheduledJobInstanceResponse struct {
	ID                string          `json:"id"`
	ScheduledJobID    string          `json:"scheduledJobId"`
	ClientID          string          `json:"clientId,omitempty"`
	JobCode           string          `json:"jobCode"`
	TriggerKind       string          `json:"triggerKind"`
	ScheduledFor      string          `json:"scheduledFor,omitempty"`
	FiredAt           string          `json:"firedAt"`
	DeliveredAt       string          `json:"deliveredAt,omitempty"`
	CompletedAt       string          `json:"completedAt,omitempty"`
	Status            string          `json:"status"`
	DeliveryAttempts  int32           `json:"deliveryAttempts"`
	DeliveryError     string          `json:"deliveryError,omitempty"`
	CompletionStatus  string          `json:"completionStatus,omitempty"`
	CompletionResult  json.RawMessage `json:"completionResult,omitempty"`
	CorrelationID     string          `json:"correlationId,omitempty"`
	CreatedAt         string          `json:"createdAt"`
}

// InstanceLogResponse — one log line from an instance.
type InstanceLogResponse struct {
	ID         string          `json:"id"`
	InstanceID string          `json:"instanceId"`
	Level      string          `json:"level"`
	Message    string          `json:"message"`
	Metadata   json.RawMessage `json:"metadata,omitempty"`
	CreatedAt  string          `json:"createdAt"`
}

// ScheduledJobListResponse — paginated list for List().
type ScheduledJobListResponse struct {
	Data       []ScheduledJobResponse `json:"data"`
	Page       uint32                 `json:"page"`
	Size       uint32                 `json:"size"`
	Total      uint64                 `json:"total"`
	TotalPages uint32                 `json:"totalPages"`
}

// ScheduledJobInstanceListResponse — paginated list for ListInstances().
type ScheduledJobInstanceListResponse struct {
	Data       []ScheduledJobInstanceResponse `json:"data"`
	Page       uint32                         `json:"page"`
	Size       uint32                         `json:"size"`
	Total      uint64                         `json:"total"`
	TotalPages uint32                         `json:"totalPages"`
}

// InstanceLogListResponse — GET /api/scheduled-jobs/instances/{id}/logs.
type InstanceLogListResponse struct {
	Logs  []InstanceLogResponse `json:"logs"`
	Total uint64                `json:"total,omitempty"`
}

// FireResponse — { instanceId } from a manual fire.
type FireResponse struct {
	InstanceID string `json:"instanceId"`
}

// ─── Sync DTOs ───────────────────────────────────────────────────────

// SyncScheduledJobsRequest — body for the per-app sync endpoint.
// ClientID empty syncs platform-scoped jobs (anchor only).
// ArchiveUnlisted archives jobs not in the list (note: this is in the
// body, not the query — distinct from other sync endpoints).
type SyncScheduledJobsRequest struct {
	ClientID        string                  `json:"clientId,omitempty"`
	Jobs            []SyncScheduledJobItem  `json:"jobs"`
	ArchiveUnlisted bool                    `json:"archiveUnlisted,omitempty"`
}

// SyncScheduledJobItem — one entry in the sync payload.
type SyncScheduledJobItem struct {
	Code                string          `json:"code"`
	Name                string          `json:"name"`
	Description         string          `json:"description,omitempty"`
	Crons               []string        `json:"crons"`
	Timezone            string          `json:"timezone,omitempty"`
	Payload             json.RawMessage `json:"payload,omitempty"`
	Concurrent          bool            `json:"concurrent,omitempty"`
	TracksCompletion    bool            `json:"tracksCompletion,omitempty"`
	TimeoutSeconds      *int32          `json:"timeoutSeconds,omitempty"`
	// DeliveryMaxAttempts: defaults to 3 server-side when omitted (nil).
	DeliveryMaxAttempts *int32 `json:"deliveryMaxAttempts,omitempty"`
	TargetURL           string `json:"targetUrl,omitempty"`
}

// SyncScheduledJobsResult — distinct shape from other sync endpoints:
// returns per-code lists rather than counts.
type SyncScheduledJobsResult struct {
	ApplicationCode string   `json:"applicationCode"`
	Created         []string `json:"created"`
	Updated         []string `json:"updated"`
	Archived        []string `json:"archived"`
}

// ─── Resource ────────────────────────────────────────────────────────

// ScheduledJobsResource — /api/scheduled-jobs/*.
type ScheduledJobsResource struct {
	c *FlowCatalystClient
}

// Create — POST /api/scheduled-jobs. Returns { id } only.
func (r *ScheduledJobsResource) Create(ctx context.Context, req *CreateScheduledJobRequest) (*CreatedResponse, error) {
	var out CreatedResponse
	if err := r.c.Post(ctx, "/api/scheduled-jobs", req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// List — GET /api/scheduled-jobs with filters + pagination.
func (r *ScheduledJobsResource) List(ctx context.Context, filters *ScheduledJobFilters) (*ScheduledJobListResponse, error) {
	q := ""
	if filters != nil {
		q = NewQuery().
			String("clientId", filters.ClientID).
			String("status", filters.Status).
			String("search", filters.Search).
			Uint32("page", filters.Page).
			Uint32("size", filters.Size).
			Encode()
	}
	var out ScheduledJobListResponse
	if err := r.c.Get(ctx, "/api/scheduled-jobs"+q, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Get — GET /api/scheduled-jobs/{id}.
func (r *ScheduledJobsResource) Get(ctx context.Context, id string) (*ScheduledJobResponse, error) {
	var out ScheduledJobResponse
	if err := r.c.Get(ctx, "/api/scheduled-jobs/"+id, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// GetByCode — GET /api/scheduled-jobs/by-code/{code}. Optional client scope.
func (r *ScheduledJobsResource) GetByCode(ctx context.Context, code, clientID string) (*ScheduledJobResponse, error) {
	q := NewQuery().String("clientId", clientID).Encode()
	var out ScheduledJobResponse
	if err := r.c.Get(ctx, "/api/scheduled-jobs/by-code/"+code+q, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Update — PUT /api/scheduled-jobs/{id}.
func (r *ScheduledJobsResource) Update(ctx context.Context, id string, req *UpdateScheduledJobRequest) (*ScheduledJobResponse, error) {
	var out ScheduledJobResponse
	if err := r.c.Put(ctx, "/api/scheduled-jobs/"+id, req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Pause — POST /api/scheduled-jobs/{id}/pause.
func (r *ScheduledJobsResource) Pause(ctx context.Context, id string) (*ScheduledJobResponse, error) {
	var out ScheduledJobResponse
	if err := r.c.Post(ctx, "/api/scheduled-jobs/"+id+"/pause", nil, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Resume — POST /api/scheduled-jobs/{id}/resume.
func (r *ScheduledJobsResource) Resume(ctx context.Context, id string) (*ScheduledJobResponse, error) {
	var out ScheduledJobResponse
	if err := r.c.Post(ctx, "/api/scheduled-jobs/"+id+"/resume", nil, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Archive — POST /api/scheduled-jobs/{id}/archive (soft delete, kept for audit).
func (r *ScheduledJobsResource) Archive(ctx context.Context, id string) (*ScheduledJobResponse, error) {
	var out ScheduledJobResponse
	if err := r.c.Post(ctx, "/api/scheduled-jobs/"+id+"/archive", nil, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Delete — DELETE /api/scheduled-jobs/{id} (hard delete).
func (r *ScheduledJobsResource) Delete(ctx context.Context, id string) error {
	return r.c.Delete(ctx, "/api/scheduled-jobs/"+id, nil)
}

// Fire — POST /api/scheduled-jobs/{id}/fire. Returns the new instance id.
func (r *ScheduledJobsResource) Fire(ctx context.Context, id string, req *FireRequest) (*FireResponse, error) {
	if req == nil {
		req = &FireRequest{}
	}
	var out FireResponse
	if err := r.c.Post(ctx, "/api/scheduled-jobs/"+id+"/fire", req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// ListInstances — GET /api/scheduled-jobs/{jobId}/instances with filters.
func (r *ScheduledJobsResource) ListInstances(ctx context.Context, jobID string, filters *InstanceFilters) (*ScheduledJobInstanceListResponse, error) {
	q := ""
	if filters != nil {
		q = NewQuery().
			String("status", filters.Status).
			String("triggerKind", filters.TriggerKind).
			String("from", filters.From).
			String("to", filters.To).
			Uint32("page", filters.Page).
			Uint32("size", filters.Size).
			Encode()
	}
	var out ScheduledJobInstanceListResponse
	if err := r.c.Get(ctx, fmt.Sprintf("/api/scheduled-jobs/%s/instances%s", jobID, q), &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// GetInstance — GET /api/scheduled-jobs/instances/{id}.
func (r *ScheduledJobsResource) GetInstance(ctx context.Context, instanceID string) (*ScheduledJobInstanceResponse, error) {
	var out ScheduledJobInstanceResponse
	if err := r.c.Get(ctx, "/api/scheduled-jobs/instances/"+instanceID, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// ListInstanceLogs — GET /api/scheduled-jobs/instances/{id}/logs. The
// platform answers a bare array of log entries; it is returned wrapped in
// an InstanceLogListResponse (Total = number of entries).
func (r *ScheduledJobsResource) ListInstanceLogs(ctx context.Context, instanceID string) (*InstanceLogListResponse, error) {
	var logs []InstanceLogResponse
	if err := r.c.Get(ctx, "/api/scheduled-jobs/instances/"+instanceID+"/logs", &logs); err != nil {
		return nil, err
	}
	return &InstanceLogListResponse{Logs: logs, Total: uint64(len(logs))}, nil
}

// LogForInstance — SDK callback to append a log entry to a running instance.
// POST /api/scheduled-jobs/instances/{id}/log.
//
// The platform requires a level; an empty Level is sent as INFO.
func (r *ScheduledJobsResource) LogForInstance(ctx context.Context, instanceID string, req *InstanceLogRequest) (*InstanceLogResponse, error) {
	if req != nil && req.Level == "" {
		withLevel := *req
		withLevel.Level = LogLevelInfo
		req = &withLevel
	}
	var out InstanceLogResponse
	if err := r.c.Post(ctx, "/api/scheduled-jobs/instances/"+instanceID+"/log", req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// CompleteInstance — SDK callback to mark an instance complete.
// POST /api/scheduled-jobs/instances/{id}/complete.
func (r *ScheduledJobsResource) CompleteInstance(ctx context.Context, instanceID string, req *InstanceCompleteRequest) (*ScheduledJobInstanceResponse, error) {
	var out ScheduledJobInstanceResponse
	if err := r.c.Post(ctx, "/api/scheduled-jobs/instances/"+instanceID+"/complete", req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}

// Sync — POST /api/applications/{appCode}/scheduled-jobs/sync. Unlike
// other syncs, scheduled-jobs sync puts ArchiveUnlisted in the body
// (not removeUnlisted in the query) and returns per-code lists rather
// than counts.
func (r *ScheduledJobsResource) Sync(ctx context.Context, appCode string, req *SyncScheduledJobsRequest) (*SyncScheduledJobsResult, error) {
	var out SyncScheduledJobsResult
	if err := r.c.Post(ctx, "/api/applications/"+appCode+"/scheduled-jobs/sync", req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}
