package sync

import (
	"context"

	"github.com/flowcatalyst/flowcatalyst/clients/go-sdk/client"
)

// Synchronizer orchestrates per-category sync calls against a
// FlowCatalystClient. Reuse one Synchronizer across calls — the client
// is cheap to share.
//
// Categories are sync'd in a fixed order so referential FKs already
// exist by the time later categories arrive:
//
//  1. roles
//  2. event types
//  3. subscriptions
//  4. dispatch pools
//  5. principals
//  6. processes
//
// A failure in one category does NOT abort the rest — the error lands
// on the per-category Result.Error field.
type Synchronizer struct {
	client *client.FlowCatalystClient
}

// NewSynchronizer wires a Synchronizer against an existing client.
func NewSynchronizer(c *client.FlowCatalystClient) *Synchronizer {
	return &Synchronizer{client: c}
}

// Sync pushes the DefinitionSet to the platform per Options. Returns
// the aggregate result; inspect Result.HasErrors / Result.Errors to
// see what failed.
func (s *Synchronizer) Sync(ctx context.Context, set *DefinitionSet, opts Options) *Result {
	out := &Result{ApplicationCode: set.ApplicationCode}
	app := set.ApplicationCode

	if opts.SyncRoles && len(set.Roles) > 0 {
		out.Roles = s.runRoles(ctx, app, set.Roles, opts.RemoveUnlisted)
	}
	if opts.SyncEventTypes && len(set.EventTypes) > 0 {
		out.EventTypes = s.runEventTypes(ctx, app, set.EventTypes, opts.RemoveUnlisted)
	}
	if opts.SyncSubscriptions && len(set.Subscriptions) > 0 {
		out.Subscriptions = s.runSubscriptions(ctx, app, set.Subscriptions, opts.RemoveUnlisted)
	}
	if opts.SyncDispatchPools && len(set.DispatchPools) > 0 {
		out.DispatchPools = s.runDispatchPools(ctx, app, set.DispatchPools, opts.RemoveUnlisted)
	}
	if opts.SyncPrincipals && len(set.Principals) > 0 {
		out.Principals = s.runPrincipals(ctx, app, set.Principals, opts.RemoveUnlisted)
	}
	if opts.SyncProcesses && len(set.Processes) > 0 {
		out.Processes = s.runProcesses(ctx, app, set.Processes, opts.RemoveUnlisted)
	}
	return out
}

// ─── Per-category runners ────────────────────────────────────────────

func (s *Synchronizer) runRoles(ctx context.Context, app string, defs []RoleDefinition, removeUnlisted bool) *CategoryResult {
	items := make([]client.SyncRoleItem, 0, len(defs))
	for _, d := range defs {
		items = append(items, client.SyncRoleItem{
			Name:          d.Name,
			DisplayName:   d.DisplayName,
			Description:   d.Description,
			Permissions:   d.Permissions,
			ClientManaged: d.ClientManaged,
		})
	}
	res, err := s.client.Roles().Sync(ctx, app, &client.SyncRolesRequest{Roles: items}, removeUnlisted)
	return toCategoryResult(res, err)
}

func (s *Synchronizer) runEventTypes(ctx context.Context, app string, defs []EventTypeDefinition, removeUnlisted bool) *CategoryResult {
	items := make([]client.CreateEventTypeRequest, 0, len(defs))
	for _, d := range defs {
		items = append(items, client.CreateEventTypeRequest{
			Code:        d.Code,
			Name:        d.Name,
			Description: d.Description,
		})
	}
	res, err := s.client.EventTypes().Sync(ctx, app, &client.SyncEventTypesRequest{EventTypes: items}, removeUnlisted)
	return toCategoryResult(res, err)
}

func (s *Synchronizer) runSubscriptions(ctx context.Context, app string, defs []SubscriptionDefinition, removeUnlisted bool) *CategoryResult {
	items := make([]client.SyncSubscriptionItem, 0, len(defs))
	for _, d := range defs {
		bindings := make([]client.EventTypeBinding, 0, len(d.EventTypes))
		for _, b := range d.EventTypes {
			bindings = append(bindings, client.EventTypeBinding{EventTypeCode: b.EventTypeCode, Filter: b.Filter})
		}
		items = append(items, client.SyncSubscriptionItem{
			Code:             d.Code,
			Name:             d.Name,
			Description:      d.Description,
			Target:           d.Target,
			ConnectionID:     d.ConnectionID,
			DispatchPoolCode: d.DispatchPoolCode,
			Mode:             d.Mode,
			MaxRetries:       d.MaxRetries,
			TimeoutSeconds:   d.TimeoutSeconds,
			DataOnly:         d.DataOnly,
			EventTypes:       bindings,
		})
	}
	res, err := s.client.Subscriptions().Sync(ctx, app, &client.SyncSubscriptionsRequest{Subscriptions: items}, removeUnlisted)
	return toCategoryResult(res, err)
}

func (s *Synchronizer) runDispatchPools(ctx context.Context, app string, defs []DispatchPoolDefinition, removeUnlisted bool) *CategoryResult {
	items := make([]client.SyncDispatchPoolItem, 0, len(defs))
	for _, d := range defs {
		items = append(items, client.SyncDispatchPoolItem{
			Code:        d.Code,
			Name:        d.Name,
			Description: d.Description,
			Concurrency: d.Concurrency,
		})
	}
	res, err := s.client.DispatchPools().Sync(ctx, app, &client.SyncDispatchPoolsRequest{Pools: items}, removeUnlisted)
	return toCategoryResult(res, err)
}

func (s *Synchronizer) runPrincipals(ctx context.Context, app string, defs []PrincipalDefinition, removeUnlisted bool) *CategoryResult {
	items := make([]client.SyncPrincipalItem, 0, len(defs))
	for _, d := range defs {
		items = append(items, client.SyncPrincipalItem{
			Email:  d.Email,
			Name:   d.Name,
			Roles:  d.Roles,
			Active: d.Active,

			PasswordHash: d.PasswordHash,
		})
	}
	res, err := s.client.Principals().Sync(ctx, app, &client.SyncPrincipalsRequest{Principals: items}, removeUnlisted)
	return toCategoryResult(res, err)
}

func (s *Synchronizer) runProcesses(ctx context.Context, app string, defs []ProcessDefinition, removeUnlisted bool) *CategoryResult {
	items := make([]client.SyncProcessInput, 0, len(defs))
	for _, d := range defs {
		items = append(items, client.SyncProcessInput{
			Code:        d.Code,
			Name:        d.Name,
			Description: d.Description,
			Steps:       d.Steps,
		})
	}
	res, err := s.client.Processes().Sync(ctx, app, &client.SyncProcessesRequest{Processes: items}, removeUnlisted)
	return toCategoryResult(res, err)
}

func toCategoryResult(res *client.SyncResult, err error) *CategoryResult {
	if err != nil {
		return &CategoryResult{Error: err.Error()}
	}
	if res == nil {
		return &CategoryResult{}
	}
	return &CategoryResult{
		Created:     res.Created,
		Updated:     res.Updated,
		Deleted:     res.Deleted,
		SyncedCodes: res.SyncedCodes,

		PasswordHashIgnored: res.PasswordHashIgnored,
	}
}
