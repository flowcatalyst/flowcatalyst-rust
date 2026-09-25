// Package sync orchestrates declarative reconciliation of an
// application's FlowCatalyst definitions against the platform.
//
// Build a DefinitionSet with the per-category fluent builders, then
// hand it to Synchronizer.Sync. Each category becomes one HTTP call
// against the platform's per-resource sync endpoint
// (POST /api/applications/{appCode}/<resource>/sync). Failures on one
// category do NOT abort the rest — they're captured on the returned
// SyncResult so the caller sees a complete picture in one round-trip.
//
// Mirrors crates/fc-sdk/src/sync/ in shape so the four SDKs converge
// on one orchestration vocabulary.
package sync

import "encoding/json"

// RoleDefinition declares a role for an application. Name is the short
// name without the `<app>:` prefix — the platform adds it from the sync
// URL. Set ClientManaged true to let clients re-assign or remove the
// role per-tenant.
type RoleDefinition struct {
	Name          string
	DisplayName   string
	Description   string
	Permissions   []string
	ClientManaged bool
}

// MakeRole starts a RoleDefinition with the given short name.
func MakeRole(name string) RoleDefinition { return RoleDefinition{Name: name} }

func (r RoleDefinition) WithDisplayName(name string) RoleDefinition     { r.DisplayName = name; return r }
func (r RoleDefinition) WithDescription(d string) RoleDefinition        { r.Description = d; return r }
func (r RoleDefinition) WithPermissions(perms ...string) RoleDefinition { r.Permissions = append(r.Permissions, perms...); return r }
func (r RoleDefinition) ClientManagedEnabled() RoleDefinition           { r.ClientManaged = true; return r }

// EventTypeDefinition declares an event type.
// Code must be the full four-segment identifier
// {app}:{subdomain}:{aggregate}:{event}.
type EventTypeDefinition struct {
	Code        string
	Name        string
	Description string
}

func MakeEventType(code, name string) EventTypeDefinition {
	return EventTypeDefinition{Code: code, Name: name}
}
func (e EventTypeDefinition) WithDescription(d string) EventTypeDefinition { e.Description = d; return e }

// SubscriptionBinding pairs an event type pattern with an optional filter.
type SubscriptionBinding struct {
	EventTypeCode string
	Filter        string
}

// SubscriptionDefinition declares a webhook subscription.
type SubscriptionDefinition struct {
	Code             string
	Name             string
	Description      string
	Target           string // webhook endpoint URL
	ConnectionID     string
	DispatchPoolCode string
	Mode             string // "Immediate" | "BlockOnError" — see platform docs
	MaxRetries       *uint32
	TimeoutSeconds   *uint32
	DataOnly         bool
	EventTypes       []SubscriptionBinding
}

func MakeSubscription(code, name, target string) SubscriptionDefinition {
	return SubscriptionDefinition{Code: code, Name: name, Target: target}
}
func (s SubscriptionDefinition) WithDescription(d string) SubscriptionDefinition         { s.Description = d; return s }
func (s SubscriptionDefinition) WithConnection(id string) SubscriptionDefinition         { s.ConnectionID = id; return s }
func (s SubscriptionDefinition) WithDispatchPool(code string) SubscriptionDefinition     { s.DispatchPoolCode = code; return s }
func (s SubscriptionDefinition) WithMode(m string) SubscriptionDefinition                { s.Mode = m; return s }
func (s SubscriptionDefinition) WithMaxRetries(n uint32) SubscriptionDefinition          { s.MaxRetries = &n; return s }
func (s SubscriptionDefinition) WithTimeout(secs uint32) SubscriptionDefinition          { s.TimeoutSeconds = &secs; return s }
func (s SubscriptionDefinition) DataOnlyEnabled() SubscriptionDefinition                 { s.DataOnly = true; return s }
func (s SubscriptionDefinition) Bind(eventTypeCode, filter string) SubscriptionDefinition {
	s.EventTypes = append(s.EventTypes, SubscriptionBinding{EventTypeCode: eventTypeCode, Filter: filter})
	return s
}

// DispatchPoolDefinition declares a dispatch pool.
type DispatchPoolDefinition struct {
	Code        string
	Name        string
	Description string
	Concurrency uint32
}

func MakeDispatchPool(code, name string) DispatchPoolDefinition {
	return DispatchPoolDefinition{Code: code, Name: name}
}
func (d DispatchPoolDefinition) WithDescription(s string) DispatchPoolDefinition  { d.Description = s; return d }
func (d DispatchPoolDefinition) WithConcurrency(c uint32) DispatchPoolDefinition  { d.Concurrency = c; return d }

// PrincipalDefinition declares a user principal. Matched by Email on
// sync. Active defaults to true (set Inactive() to override).
type PrincipalDefinition struct {
	Email  string
	Name   string
	Roles  []string
	Active bool
	// PasswordHash — optional pre-hashed password, used only when the sync
	// creates the user (never applied to an existing one).
	PasswordHash string
}

// MakePrincipal starts a PrincipalDefinition for the given email. The
// principal defaults to Active.
func MakePrincipal(email string) PrincipalDefinition {
	return PrincipalDefinition{Email: email, Active: true}
}
func (p PrincipalDefinition) WithName(n string) PrincipalDefinition { p.Name = n; return p }
func (p PrincipalDefinition) WithRoles(roles ...string) PrincipalDefinition {
	p.Roles = append(p.Roles, roles...)
	return p
}

// WithPasswordHash sets a pre-hashed password (bcrypt, argon2) for the
// platform to store if, and only if, the sync creates this user.
func (p PrincipalDefinition) WithPasswordHash(hash string) PrincipalDefinition {
	p.PasswordHash = hash
	return p
}

// Inactive marks the principal as inactive on sync.
func (p PrincipalDefinition) Inactive() PrincipalDefinition { p.Active = false; return p }

// ProcessDefinition declares a process documentation entry.
type ProcessDefinition struct {
	Code        string
	Name        string
	Description string
	Steps       json.RawMessage
}

func MakeProcess(code, name string) ProcessDefinition { return ProcessDefinition{Code: code, Name: name} }
func (p ProcessDefinition) WithDescription(s string) ProcessDefinition { p.Description = s; return p }
func (p ProcessDefinition) WithSteps(steps json.RawMessage) ProcessDefinition { p.Steps = steps; return p }
