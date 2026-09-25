package sync

// CategoryResult is the per-category outcome of a Synchronizer.Sync.
//
// Failures are captured here rather than propagated as errors so the
// caller can see exactly which categories succeeded, which failed, and
// what was touched on the platform.
type CategoryResult struct {
	Created     uint32
	Updated     uint32
	Deleted     uint32
	SyncedCodes []string
	// PasswordHashIgnored — principals only: the existing users (emails)
	// whose PasswordHash the platform did not apply.
	PasswordHashIgnored []string
	// Error is non-empty if this category's HTTP call failed.
	Error string
}

// IsError reports whether this category failed.
func (c *CategoryResult) IsError() bool { return c.Error != "" }

// Touched is Created + Updated + Deleted — useful for one-line summaries.
func (c *CategoryResult) Touched() uint32 { return c.Created + c.Updated + c.Deleted }

// Result is the aggregate outcome of syncing a DefinitionSet. Each
// per-category field is non-nil only if the Synchronizer called that
// endpoint (i.e. the category was enabled in Options AND non-empty).
type Result struct {
	ApplicationCode string
	Roles           *CategoryResult
	EventTypes      *CategoryResult
	Subscriptions   *CategoryResult
	DispatchPools   *CategoryResult
	Principals      *CategoryResult
	Processes       *CategoryResult
}

// HasErrors reports whether any category failed.
func (r *Result) HasErrors() bool {
	for _, c := range []*CategoryResult{r.Roles, r.EventTypes, r.Subscriptions, r.DispatchPools, r.Principals, r.Processes} {
		if c != nil && c.IsError() {
			return true
		}
	}
	return false
}

// Errors returns a (category → message) list of failed categories.
func (r *Result) Errors() map[string]string {
	out := map[string]string{}
	pairs := []struct {
		name string
		res  *CategoryResult
	}{
		{"roles", r.Roles},
		{"event_types", r.EventTypes},
		{"subscriptions", r.Subscriptions},
		{"dispatch_pools", r.DispatchPools},
		{"principals", r.Principals},
		{"processes", r.Processes},
	}
	for _, p := range pairs {
		if p.res != nil && p.res.IsError() {
			out[p.name] = p.res.Error
		}
	}
	return out
}
