package client

import "context"

// CreateServiceAccountRequest — POST /api/service-accounts.
//
// A new service account has no application access. AllApplications true
// grants it every application, present and future; the platform answers
// 403 unless the caller itself reaches every application. Grant single
// applications afterwards through the principal's application access.
type CreateServiceAccountRequest struct {
	Code        string `json:"code"`
	Name        string `json:"name"`
	Description string `json:"description,omitempty"`
	// Scope — requested scope: ANCHOR, PARTNER or CLIENT. The token tier
	// follows ClientIDs (none → ANCHOR, one → CLIENT, several → PARTNER).
	Scope     string   `json:"scope,omitempty"`
	ClientIDs []string `json:"clientIds,omitempty"`
	// AllApplications grants every application. Omitted when false.
	AllApplications bool `json:"allApplications,omitempty"`
}

// ServiceAccount — a service account as the /api/service-accounts routes
// return it.
type ServiceAccount struct {
	ID            string   `json:"id"`
	Code          string   `json:"code"`
	Name          string   `json:"name"`
	Description   string   `json:"description,omitempty"`
	Scope         string   `json:"scope,omitempty"`
	ClientIDs     []string `json:"clientIds"`
	ApplicationID string   `json:"applicationId,omitempty"`
	Active        bool     `json:"active"`
	AuthType      string   `json:"authType"`
	Roles         []string `json:"roles"`
	LastUsedAt    string   `json:"lastUsedAt,omitempty"`
	CreatedAt     string   `json:"createdAt"`
	UpdatedAt     string   `json:"updatedAt"`
}

// ServiceAccountOAuthCredentials — the client_credentials pair, returned once.
type ServiceAccountOAuthCredentials struct {
	ClientID     string `json:"clientId"`
	ClientSecret string `json:"clientSecret"`
}

// ServiceAccountWebhookCredentials — the webhook bearer token and signing
// secret, returned once.
type ServiceAccountWebhookCredentials struct {
	AuthToken     string `json:"authToken"`
	SigningSecret string `json:"signingSecret"`
}

// CreateServiceAccountResponse — the new account and its one-time secrets.
type CreateServiceAccountResponse struct {
	ServiceAccount ServiceAccount                   `json:"serviceAccount"`
	PrincipalID    string                           `json:"principalId"`
	OAuth          ServiceAccountOAuthCredentials   `json:"oauth"`
	Webhook        ServiceAccountWebhookCredentials `json:"webhook"`
}

// ServiceAccountsResource — /api/service-accounts/*.
type ServiceAccountsResource struct {
	c *FlowCatalystClient
}

// Create — POST /api/service-accounts. The response carries the OAuth
// client secret and webhook credentials, which are never shown again.
func (r *ServiceAccountsResource) Create(ctx context.Context, req *CreateServiceAccountRequest) (*CreateServiceAccountResponse, error) {
	var out CreateServiceAccountResponse
	if err := r.c.Post(ctx, "/api/service-accounts", req, &out); err != nil {
		return nil, err
	}
	return &out, nil
}
