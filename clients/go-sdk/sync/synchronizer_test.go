package sync_test

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/flowcatalyst/flowcatalyst/clients/go-sdk/client"
	"github.com/flowcatalyst/flowcatalyst/clients/go-sdk/sync"
)

func TestSynchronizerSkipsEmptyCategories(t *testing.T) {
	var hits atomic.Int32
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		hits.Add(1)
		_ = json.NewEncoder(w).Encode(client.SyncResult{ApplicationCode: "x"})
	}))
	defer srv.Close()

	c := client.New(srv.URL)
	s := sync.NewSynchronizer(c)
	set := sync.ForApplication("orders") // no categories populated
	out := s.Sync(context.Background(), set, sync.DefaultOptions())

	assert.Equal(t, int32(0), hits.Load(), "no category enabled → no HTTP calls")
	assert.False(t, out.HasErrors())
}

func TestSynchronizerRunsRolesOnlyWhenConfigured(t *testing.T) {
	var paths []string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		paths = append(paths, r.URL.Path)
		_ = json.NewEncoder(w).Encode(client.SyncResult{
			ApplicationCode: "orders",
			Created:         1,
			SyncedCodes:     []string{"admin"},
		})
	}))
	defer srv.Close()

	c := client.New(srv.URL)
	s := sync.NewSynchronizer(c)
	set := sync.ForApplication("orders").AddRole(
		sync.MakeRole("admin").WithDisplayName("Admin"),
	)
	out := s.Sync(context.Background(), set, sync.RolesOnly())

	require.NotNil(t, out.Roles)
	assert.Equal(t, uint32(1), out.Roles.Created)
	assert.Equal(t, []string{"admin"}, out.Roles.SyncedCodes)
	assert.False(t, out.HasErrors())
	assert.Equal(t, []string{"/api/applications/orders/roles/sync"}, paths)
}

func TestSynchronizerCapturesPerCategoryError(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusForbidden)
		_, _ = w.Write([]byte(`{"code":"DENY","message":"nope"}`))
	}))
	defer srv.Close()

	c := client.New(srv.URL, client.WithRetry(1, 0)) // skip retry to keep the test fast
	s := sync.NewSynchronizer(c)
	set := sync.ForApplication("orders").AddRole(sync.MakeRole("admin"))
	out := s.Sync(context.Background(), set, sync.RolesOnly())

	require.NotNil(t, out.Roles)
	assert.True(t, out.HasErrors())
	assert.Contains(t, out.Roles.Error, "403")
	errs := out.Errors()
	assert.Equal(t, 1, len(errs))
	assert.Contains(t, errs, "roles")
}

func TestSynchronizerSendsRemoveUnlistedQuery(t *testing.T) {
	var seen string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		seen = r.URL.RawQuery
		_ = json.NewEncoder(w).Encode(client.SyncResult{})
	}))
	defer srv.Close()

	c := client.New(srv.URL)
	s := sync.NewSynchronizer(c)
	set := sync.ForApplication("orders").AddRole(sync.MakeRole("admin"))
	_ = s.Sync(context.Background(), set, sync.Options{SyncRoles: true, RemoveUnlisted: true})

	assert.Equal(t, "removeUnlisted=true", seen)
}

// Owner decision 22 of 2026-09-25: the principal sync carries a passwordHash
// for users it creates, and reports the existing users whose hash it ignored.
func TestSynchronizerPrincipalsCarryPasswordHashAndReportIgnored(t *testing.T) {
	var body map[string][]map[string]any
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewDecoder(r.Body).Decode(&body)
		_ = json.NewEncoder(w).Encode(client.SyncResult{
			ApplicationCode:     "hr",
			Updated:             1,
			SyncedCodes:         []string{"a@example.com"},
			PasswordHashIgnored: []string{"a@example.com"},
		})
	}))
	defer srv.Close()

	s := sync.NewSynchronizer(client.New(srv.URL))
	set := sync.ForApplication("hr").AddPrincipal(
		sync.MakePrincipal("a@example.com").WithName("A").WithPasswordHash("$2y$10$hash"),
	)
	out := s.Sync(context.Background(), set, sync.PrincipalsOnly())

	require.NotNil(t, out.Principals)
	assert.Equal(t, []string{"a@example.com"}, out.Principals.PasswordHashIgnored)
	require.Len(t, body["principals"], 1)
	assert.Equal(t, "$2y$10$hash", body["principals"][0]["passwordHash"])
}
