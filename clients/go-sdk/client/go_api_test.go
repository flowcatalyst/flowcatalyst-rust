package client_test

// Tests that pin the client to the Go platform's wire contract
// (flowcatalyst-go api/openapi.lock.json): paths, methods, request
// members and response shapes.

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"sort"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/flowcatalyst/flowcatalyst/clients/go-sdk/client"
)

// newStatusSrv is newMockSrv with an explicit status code (e.g. 204).
func newStatusSrv(t *testing.T, status int, respJSON string) (*httptest.Server, *seenRequest) {
	t.Helper()
	seen := &seenRequest{}
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		seen.method = r.Method
		seen.path = r.URL.Path
		seen.query = r.URL.Query()
		if r.Body != nil {
			b, _ := io.ReadAll(r.Body)
			seen.body = string(b)
		}
		if respJSON != "" {
			w.Header().Set("Content-Type", "application/json")
		}
		w.WriteHeader(status)
		if respJSON != "" {
			_, _ = w.Write([]byte(respJSON))
		}
	}))
	t.Cleanup(srv.Close)
	return srv, seen
}

// routeSrv answers per "METHOD path" and records every request in order.
func routeSrv(t *testing.T, routes map[string]string) (*httptest.Server, *[]string) {
	t.Helper()
	var calls []string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		key := r.Method + " " + r.URL.Path
		calls = append(calls, key)
		body, ok := routes[key]
		if !ok {
			w.WriteHeader(http.StatusNotFound)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(body))
	}))
	t.Cleanup(srv.Close)
	return srv, &calls
}

// bodyKeys returns the sorted top-level member names of a JSON object.
func bodyKeys(t *testing.T, raw string) []string {
	t.Helper()
	var m map[string]json.RawMessage
	require.NoError(t, json.Unmarshal([]byte(raw), &m))
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return keys
}

// itemKeys returns the member names of every object in body[field].
func itemKeys(t *testing.T, raw, field string) [][]string {
	t.Helper()
	var m map[string][]map[string]json.RawMessage
	require.NoError(t, json.Unmarshal([]byte(raw), &m))
	var out [][]string
	for _, item := range m[field] {
		keys := make([]string, 0, len(item))
		for k := range item {
			keys = append(keys, k)
		}
		sort.Strings(keys)
		out = append(out, keys)
	}
	return out
}

// ─── Applications ────────────────────────────────────────────────────

const saJSON = `{"id":"sa_1","code":"orders-svc","name":"Orders","active":true,"clientIds":[],"authType":"BEARER_TOKEN","roles":["orders:admin"],"principalId":"prn_sa","oauthClientId":"oac_1","applicationId":"app_1","scope":"ANCHOR","createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z"}`

func TestApplicationsGetServiceAccountReadsApplicationThenAccount(t *testing.T) {
	srv, calls := routeSrv(t, map[string]string{
		"GET /api/applications/app_1":    `{"id":"app_1","type":"APPLICATION","code":"orders","name":"Orders","active":true,"hasLoginClient":false,"serviceAccountId":"sa_1","createdAt":"","updatedAt":""}`,
		"GET /api/service-accounts/sa_1": saJSON,
	})
	c := client.New(srv.URL)

	sa, err := c.Applications().GetServiceAccount(context.Background(), "app_1")
	require.NoError(t, err)
	assert.Equal(t, []string{"GET /api/applications/app_1", "GET /api/service-accounts/sa_1"}, *calls)
	assert.Equal(t, "sa_1", sa.ID)
	assert.Equal(t, "prn_sa", sa.PrincipalID)
	assert.Equal(t, "oac_1", sa.OAuthClientID)
	assert.Equal(t, []string{"orders:admin"}, sa.Roles)
}

func TestApplicationsGetServiceAccountNotFoundWithoutOne(t *testing.T) {
	srv, calls := routeSrv(t, map[string]string{
		"GET /api/applications/app_1": `{"id":"app_1","type":"APPLICATION","code":"orders","name":"Orders","active":true,"hasLoginClient":false,"createdAt":"","updatedAt":""}`,
	})
	c := client.New(srv.URL)

	_, err := c.Applications().GetServiceAccount(context.Background(), "app_1")
	require.Error(t, err)
	var apiErr *client.APIError
	require.True(t, errors.As(err, &apiErr))
	assert.True(t, apiErr.IsNotFound())
	assert.Equal(t, "SERVICE_ACCOUNT_NOT_FOUND", apiErr.Code())
	assert.Equal(t, []string{"GET /api/applications/app_1"}, *calls, "no service-account lookup without an id")
}

func TestApplicationsGetClientConfig(t *testing.T) {
	srv, seen := newMockSrv(t, `{"id":"acc_1","applicationId":"app_1","clientId":"clt_1","enabled":true,"baseUrlOverride":"https://acme.example","configJson":{"tier":"gold"},"createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-02T00:00:00Z"}`)
	c := client.New(srv.URL)

	cfg, err := c.Applications().GetClientConfig(context.Background(), "app_1", "clt_1")
	require.NoError(t, err)
	assert.Equal(t, http.MethodGet, seen.method)
	assert.Equal(t, "/api/applications/app_1/clients/clt_1", seen.path)
	assert.True(t, cfg.Enabled)
	assert.Equal(t, "https://acme.example", cfg.BaseURLOverride)
	assert.JSONEq(t, `{"tier":"gold"}`, string(cfg.ConfigJSON))
	assert.Equal(t, "2026-01-02T00:00:00Z", cfg.UpdatedAt)
}

func TestApplicationsListClientsReadsItems(t *testing.T) {
	srv, seen := newMockSrv(t, `{"items":[{"id":"acc_1","applicationId":"app_1","clientId":"clt_1","enabled":false,"configJson":{"a":1},"createdAt":"","updatedAt":""}]}`)
	c := client.New(srv.URL)

	out, err := c.Applications().ListClients(context.Background(), "app_1")
	require.NoError(t, err)
	assert.Equal(t, "/api/applications/app_1/clients", seen.path)
	require.Len(t, out.Items, 1)
	assert.Equal(t, "clt_1", out.Items[0].ClientID)
	assert.JSONEq(t, `{"a":1}`, string(out.Items[0].ConfigJSON))
}

func TestApplicationsUpdateClientConfigDeprecatedStillPuts(t *testing.T) {
	srv, seen := newMockSrv(t, `{"id":"acc_1","applicationId":"app_1","clientId":"clt_1","enabled":true}`)
	c := client.New(srv.URL)

	enabled := true
	//lint:ignore SA1019 exercising the deprecated Rust-only route
	_, err := c.Applications().UpdateClientConfig(context.Background(), "app_1", "clt_1", &client.ClientConfigRequest{Enabled: &enabled})
	require.NoError(t, err)
	assert.Equal(t, http.MethodPut, seen.method)
	assert.Equal(t, "/api/applications/app_1/clients/clt_1", seen.path)
}

func TestApplicationsListSendsGoFilters(t *testing.T) {
	srv, seen := newMockSrv(t, `{"applications":[{"id":"app_1","type":"APPLICATION","code":"orders","name":"Orders","active":true,"hasLoginClient":true,"createdAt":"","updatedAt":""}],"total":1}`)
	c := client.New(srv.URL)

	active := true
	out, err := c.Applications().List(context.Background(), &active, "APPLICATION")
	require.NoError(t, err)
	assert.Equal(t, "true", seen.query.Get("active"))
	assert.Equal(t, "APPLICATION", seen.query.Get("type"))
	assert.Len(t, seen.query, 2)
	require.Len(t, out.Applications, 1)
	assert.True(t, out.Applications[0].HasLoginClient)
}

func TestApplicationsUpdateIsNoContent(t *testing.T) {
	srv, seen := newStatusSrv(t, http.StatusNoContent, ``)
	c := client.New(srv.URL)

	name := "Orders 2"
	require.NoError(t, c.Applications().Update(context.Background(), "app_1", &client.UpdateApplicationRequest{Name: &name}))
	assert.Equal(t, http.MethodPut, seen.method)
	assert.Equal(t, "/api/applications/app_1", seen.path)
	assert.JSONEq(t, `{"name":"Orders 2"}`, seen.body)
}

func TestApplicationsAttachServiceAccount(t *testing.T) {
	srv, seen := newStatusSrv(t, http.StatusNoContent, ``)
	c := client.New(srv.URL)

	require.NoError(t, c.Applications().AttachServiceAccount(context.Background(), "app_1",
		&client.AttachServiceAccountRequest{ServiceAccountID: "sa_1", ServiceAccountCode: "orders-svc"}))
	assert.Equal(t, http.MethodPost, seen.method)
	assert.Equal(t, "/api/applications/app_1/service-account", seen.path)
	assert.JSONEq(t, `{"serviceAccountId":"sa_1","serviceAccountCode":"orders-svc"}`, seen.body)
}

func TestServiceAccountsGet(t *testing.T) {
	srv, seen := newMockSrv(t, saJSON)
	c := client.New(srv.URL)

	sa, err := c.ServiceAccounts().Get(context.Background(), "sa_1")
	require.NoError(t, err)
	assert.Equal(t, http.MethodGet, seen.method)
	assert.Equal(t, "/api/service-accounts/sa_1", seen.path)
	assert.Equal(t, "BEARER_TOKEN", sa.AuthType)
	assert.Equal(t, "app_1", sa.ApplicationID)
}

// ─── Event types ─────────────────────────────────────────────────────

func TestEventTypesUpdateAlwaysSendsName(t *testing.T) {
	srv, seen := newStatusSrv(t, http.StatusNoContent, ``)
	c := client.New(srv.URL)

	require.NoError(t, c.EventTypes().Update(context.Background(), "evt_1", &client.UpdateEventTypeRequest{Name: "Order Shipped"}))
	assert.Equal(t, http.MethodPut, seen.method)
	assert.Equal(t, "/api/event-types/evt_1", seen.path)
	assert.JSONEq(t, `{"name":"Order Shipped"}`, seen.body)

	// The zero value still carries the member: the platform requires it.
	desc := "d"
	require.NoError(t, c.EventTypes().Update(context.Background(), "evt_1", &client.UpdateEventTypeRequest{Description: &desc}))
	assert.JSONEq(t, `{"name":"","description":"d"}`, seen.body)
}

func TestEventTypesCreateReturnsID(t *testing.T) {
	srv, seen := newStatusSrv(t, http.StatusCreated, `{"id":"evt_new"}`)
	c := client.New(srv.URL)

	out, err := c.EventTypes().Create(context.Background(), &client.CreateEventTypeRequest{Code: "orders:fulfilment:order:shipped", Name: "Shipped"})
	require.NoError(t, err)
	assert.Equal(t, http.MethodPost, seen.method)
	assert.Equal(t, "/api/event-types", seen.path)
	assert.Equal(t, "evt_new", out.ID)
}

func TestEventTypesGetDecodesGoFieldNames(t *testing.T) {
	srv, _ := newMockSrv(t, `{"id":"evt_1","code":"orders:fulfilment:order:shipped","name":"Shipped","application":"orders","subdomain":"fulfilment","aggregate":"order","eventName":"shipped","status":"CURRENT","source":"API","clientId":"clt_1","createdBy":"prn_1","specVersions":[{"version":"1.0","schema":{"type":"object"},"status":"CURRENT","createdAt":"2026-01-01T00:00:00Z"}],"createdAt":"","updatedAt":""}`)
	c := client.New(srv.URL)

	et, err := c.EventTypes().Get(context.Background(), "evt_1")
	require.NoError(t, err)
	assert.Equal(t, "shipped", et.EventName)
	assert.Equal(t, "API", et.Source)
	assert.Equal(t, "fulfilment", et.Subdomain)
	assert.Equal(t, "clt_1", et.ClientID)
	require.Len(t, et.SpecVersions, 1)
	assert.Equal(t, "2026-01-01T00:00:00Z", et.SpecVersions[0].CreatedAt)
}

func TestEventTypesDeleteIsDelete(t *testing.T) {
	srv, seen := newStatusSrv(t, http.StatusNoContent, ``)
	c := client.New(srv.URL)

	require.NoError(t, c.EventTypes().Delete(context.Background(), "evt_1"))
	assert.Equal(t, http.MethodDelete, seen.method)
	assert.Equal(t, "/api/event-types/evt_1", seen.path)
}

func TestEventTypesSyncSendsOnlyStrictMembers(t *testing.T) {
	srv, seen := newMockSrv(t, `{"applicationCode":"orders","created":1,"updated":0,"deleted":0,"syncedCodes":["orders:fulfilment:order:shipped"]}`)
	c := client.New(srv.URL)

	_, err := c.EventTypes().Sync(context.Background(), "orders", &client.SyncEventTypesRequest{EventTypes: []client.SyncEventTypeItem{
		{Code: "orders:fulfilment:order:shipped", Name: "Shipped", Description: "d"},
		{Code: "orders:fulfilment:order:packed", Name: "Packed"},
	}}, true)
	require.NoError(t, err)
	assert.Equal(t, "/api/applications/orders/event-types/sync", seen.path)
	assert.Equal(t, "true", seen.query.Get("removeUnlisted"))
	assert.Equal(t, [][]string{{"code", "description", "name"}, {"code", "name"}}, itemKeys(t, seen.body, "eventTypes"))
}

// ─── Connections ─────────────────────────────────────────────────────

func TestConnectionsUpdateAlwaysSendsName(t *testing.T) {
	srv, seen := newStatusSrv(t, http.StatusNoContent, ``)
	c := client.New(srv.URL)

	status := "PAUSED"
	require.NoError(t, c.Connections().Update(context.Background(), "con_1", &client.UpdateConnectionRequest{Name: "Acme", Status: &status}))
	assert.Equal(t, http.MethodPut, seen.method)
	assert.Equal(t, "/api/connections/con_1", seen.path)
	assert.JSONEq(t, `{"name":"Acme","status":"PAUSED"}`, seen.body)
}

func TestConnectionsCreateDecodesConnection(t *testing.T) {
	srv, seen := newStatusSrv(t, http.StatusCreated, `{"id":"con_1","code":"acme","name":"Acme","status":"ACTIVE","serviceAccountId":"sa_1","source":"API","applicationCode":"orders","createdAt":"","updatedAt":""}`)
	c := client.New(srv.URL)

	out, err := c.Connections().Create(context.Background(), &client.CreateConnectionRequest{Code: "acme", Name: "Acme", ServiceAccountID: "sa_1"})
	require.NoError(t, err)
	assert.Equal(t, "/api/connections", seen.path)
	assert.Equal(t, "con_1", out.ID)
	assert.Equal(t, "API", out.Source)
	assert.Equal(t, "orders", out.ApplicationCode)
}

// ─── Processes ───────────────────────────────────────────────────────

func TestProcessesCreateSendsBodyDiagramTypeAndTags(t *testing.T) {
	srv, seen := newStatusSrv(t, http.StatusCreated, `{"id":"prc_1"}`)
	c := client.New(srv.URL)

	out, err := c.Processes().Create(context.Background(), &client.CreateProcessRequest{
		Code: "orders:fulfilment:ship", Name: "Ship", Body: "graph TD; A-->B", DiagramType: "mermaid", Tags: []string{"core"},
	})
	require.NoError(t, err)
	assert.Equal(t, http.MethodPost, seen.method)
	assert.Equal(t, "/api/processes", seen.path)
	assert.Equal(t, "prc_1", out.ID)
	assert.Equal(t, []string{"body", "code", "diagramType", "name", "tags"}, bodyKeys(t, seen.body))
}

func TestProcessesUpdateIsNoContentAndCanClearTags(t *testing.T) {
	srv, seen := newStatusSrv(t, http.StatusNoContent, ``)
	c := client.New(srv.URL)

	body := "graph LR; X-->Y"
	empty := []string{}
	require.NoError(t, c.Processes().Update(context.Background(), "prc_1", &client.UpdateProcessRequest{Body: &body, Tags: &empty}))
	assert.Equal(t, http.MethodPut, seen.method)
	assert.Equal(t, "/api/processes/prc_1", seen.path)
	assert.JSONEq(t, `{"body":"graph LR; X-->Y","tags":[]}`, seen.body)
}

func TestProcessesGetDecodesGoShape(t *testing.T) {
	srv, _ := newMockSrv(t, `{"id":"prc_1","code":"orders:fulfilment:ship","name":"Ship","status":"CURRENT","source":"SDK","application":"orders","subdomain":"fulfilment","processName":"ship","body":"graph TD; A-->B","diagramType":"mermaid","tags":["core"],"createdAt":"","updatedAt":""}`)
	c := client.New(srv.URL)

	p, err := c.Processes().Get(context.Background(), "prc_1")
	require.NoError(t, err)
	assert.Equal(t, "graph TD; A-->B", p.Body)
	assert.Equal(t, "mermaid", p.DiagramType)
	assert.Equal(t, []string{"core"}, p.Tags)
	assert.Equal(t, "ship", p.ProcessName)
	assert.Equal(t, "SDK", p.Source)
}

func TestProcessesListSendsGoFilters(t *testing.T) {
	srv, seen := newMockSrv(t, `{"items":[]}`)
	c := client.New(srv.URL)

	_, err := c.Processes().List(context.Background(), &client.ProcessFilters{Application: "orders", Subdomain: "fulfilment", Status: "CURRENT"})
	require.NoError(t, err)
	assert.Equal(t, "/api/processes", seen.path)
	assert.Equal(t, "orders", seen.query.Get("application"))
	assert.Equal(t, "fulfilment", seen.query.Get("subdomain"))
	assert.Equal(t, "CURRENT", seen.query.Get("status"))
	assert.Len(t, seen.query, 3)
}

func TestProcessesArchiveIsPostAndDeleteIsPlainDelete(t *testing.T) {
	srv, seen := newStatusSrv(t, http.StatusNoContent, ``)
	c := client.New(srv.URL)

	require.NoError(t, c.Processes().Archive(context.Background(), "prc_1"))
	assert.Equal(t, http.MethodPost, seen.method)
	assert.Equal(t, "/api/processes/prc_1/archive", seen.path)

	require.NoError(t, c.Processes().Delete(context.Background(), "prc_1"))
	assert.Equal(t, http.MethodDelete, seen.method)
	assert.Equal(t, "/api/processes/prc_1", seen.path)
	assert.Empty(t, seen.query)
}

func TestProcessesSyncSendsOnlyStrictMembers(t *testing.T) {
	srv, seen := newMockSrv(t, `{"applicationCode":"orders","created":1,"updated":0,"deleted":0,"syncedCodes":["orders:fulfilment:ship"]}`)
	c := client.New(srv.URL)

	_, err := c.Processes().Sync(context.Background(), "orders", &client.SyncProcessesRequest{Processes: []client.SyncProcessInput{
		{Code: "orders:fulfilment:ship", Name: "Ship", Description: "d", Body: "graph TD;", DiagramType: "mermaid", Tags: []string{"a"}},
	}}, false)
	require.NoError(t, err)
	assert.Equal(t, "/api/applications/orders/processes/sync", seen.path)
	assert.Equal(t, [][]string{{"body", "code", "description", "diagramType", "name", "tags"}}, itemKeys(t, seen.body, "processes"))
}

// ─── Dispatch pools ──────────────────────────────────────────────────

func TestDispatchPoolsListReadsPools(t *testing.T) {
	srv, seen := newMockSrv(t, `{"pools":[{"id":"dpl_1","code":"default","name":"Default","concurrency":10,"rateLimit":600,"status":"ACTIVE","createdAt":"","updatedAt":""}],"total":1}`)
	c := client.New(srv.URL)

	out, err := c.DispatchPools().List(context.Background(), "clt_1", "ACTIVE")
	require.NoError(t, err)
	assert.Equal(t, "/api/dispatch-pools", seen.path)
	assert.Equal(t, "clt_1", seen.query.Get("clientId"))
	assert.Equal(t, "ACTIVE", seen.query.Get("status"))
	require.Len(t, out.Pools, 1)
	assert.Equal(t, uint64(1), out.Total)
	assert.Equal(t, uint32(10), out.Pools[0].Concurrency)
	require.NotNil(t, out.Pools[0].RateLimit)
	assert.Equal(t, uint32(600), *out.Pools[0].RateLimit)
}

func TestDispatchPoolsCreateReturnsID(t *testing.T) {
	srv, seen := newStatusSrv(t, http.StatusCreated, `{"id":"dpl_new"}`)
	c := client.New(srv.URL)

	out, err := c.DispatchPools().Create(context.Background(), &client.CreateDispatchPoolRequest{Code: "p", Name: "P"})
	require.NoError(t, err)
	assert.Equal(t, http.MethodPost, seen.method)
	assert.Equal(t, "dpl_new", out.ID)
}

func TestDispatchPoolsLifecycleRoutes(t *testing.T) {
	srv, seen := newStatusSrv(t, http.StatusNoContent, ``)
	c := client.New(srv.URL)
	ctx := context.Background()

	name := "Renamed"
	require.NoError(t, c.DispatchPools().Update(ctx, "dpl_1", &client.UpdateDispatchPoolRequest{Name: &name}))
	assert.Equal(t, http.MethodPut, seen.method)
	assert.Equal(t, "/api/dispatch-pools/dpl_1", seen.path)

	require.NoError(t, c.DispatchPools().Archive(ctx, "dpl_1"))
	assert.Equal(t, http.MethodPost, seen.method)
	assert.Equal(t, "/api/dispatch-pools/dpl_1/archive", seen.path)

	require.NoError(t, c.DispatchPools().Suspend(ctx, "dpl_1"))
	assert.Equal(t, http.MethodPost, seen.method)
	assert.Equal(t, "/api/dispatch-pools/dpl_1/suspend", seen.path)

	require.NoError(t, c.DispatchPools().Activate(ctx, "dpl_1"))
	assert.Equal(t, http.MethodPost, seen.method)
	assert.Equal(t, "/api/dispatch-pools/dpl_1/activate", seen.path)

	require.NoError(t, c.DispatchPools().Delete(ctx, "dpl_1"))
	assert.Equal(t, http.MethodDelete, seen.method)
	assert.Equal(t, "/api/dispatch-pools/dpl_1", seen.path)
}

// ─── Principals ──────────────────────────────────────────────────────

func TestPrincipalsFindByEmailSendsQAndKeepsExactMatches(t *testing.T) {
	srv, seen := newMockSrv(t, `{"principals":[
		{"id":"prn_1","type":"USER","scope":"CLIENT","name":"Ann","active":true,"email":"Ann@Example.com","roles":[],"isAnchorUser":false,"grantedClientIds":[],"createdAt":"","updatedAt":"","hasDeveloperCredential":false},
		{"id":"prn_2","type":"USER","scope":"CLIENT","name":"Joann","active":true,"email":"joann@example.com","roles":[],"isAnchorUser":false,"grantedClientIds":[],"createdAt":"","updatedAt":"","hasDeveloperCredential":false}
	],"total":2}`)
	c := client.New(srv.URL)

	out, err := c.Principals().FindByEmail(context.Background(), "ann@example.com")
	require.NoError(t, err)
	assert.Equal(t, "/api/principals", seen.path)
	assert.Equal(t, "ann@example.com", seen.query.Get("q"))
	assert.Empty(t, seen.query.Get("email"))
	require.Len(t, out.Principals, 1)
	assert.Equal(t, "prn_1", out.Principals[0].ID)
	assert.Equal(t, uint64(1), out.Total)
}

func TestPrincipalsListSendsGoFilters(t *testing.T) {
	srv, seen := newMockSrv(t, `{"principals":[],"total":0}`)
	c := client.New(srv.URL)

	page, size := uint32(0), uint32(20)
	_, err := c.Principals().List(context.Background(), &client.PrincipalFilters{
		Type: "USER", ClientID: "clt_1", Active: "true", Q: "ann",
		Roles: []string{"orders:admin", "orders:viewer"}, Page: &page, PageSize: &size,
		SortField: "email", SortOrder: "desc",
	})
	require.NoError(t, err)
	assert.Equal(t, "USER", seen.query.Get("type"))
	assert.Equal(t, "clt_1", seen.query.Get("clientId"))
	assert.Equal(t, "true", seen.query.Get("active"))
	assert.Equal(t, "ann", seen.query.Get("q"))
	assert.Equal(t, "orders:admin,orders:viewer", seen.query.Get("roles"))
	assert.Equal(t, "0", seen.query.Get("page"))
	assert.Equal(t, "20", seen.query.Get("pageSize"))
	assert.Equal(t, "email", seen.query.Get("sortField"))
	assert.Equal(t, "desc", seen.query.Get("sortOrder"))
}

func TestPrincipalsUpdateSendsGoMembers(t *testing.T) {
	srv, seen := newMockSrv(t, `{"id":"prn_1","type":"USER","scope":"CLIENT","name":"Ann B","active":true,"roles":[],"isAnchorUser":false,"grantedClientIds":[],"createdAt":"","updatedAt":"","hasDeveloperCredential":false}`)
	c := client.New(srv.URL)

	name, email := "Ann B", "ann@example.com"
	out, err := c.Principals().Update(context.Background(), "prn_1", &client.UpdatePrincipalRequest{Name: &name, Email: &email})
	require.NoError(t, err)
	assert.Equal(t, http.MethodPut, seen.method)
	assert.JSONEq(t, `{"name":"Ann B","email":"ann@example.com"}`, seen.body)
	assert.Equal(t, "Ann B", out.Name)
}

// ─── Audit logs ──────────────────────────────────────────────────────

func TestAuditLogsCursorPaging(t *testing.T) {
	srv, seen := newMockSrv(t, `{"auditLogs":[{"id":"aud_1","entityType":"Principal","entityId":"prn_1","operation":"CreateUser","operationJson":"{}","performedAt":"2026-01-01T00:00:00Z"}],"hasMore":true,"nextCursor":"cur_2"}`)
	c := client.New(srv.URL)

	out, err := c.AuditLogs().List(context.Background(), &client.AuditLogFilters{
		After: "cur_1", ClientIDs: []string{"clt_1", "clt_2"}, ApplicationIDs: []string{"app_1"},
		PrincipalID: "prn_9", EntityID: "prn_1",
	})
	require.NoError(t, err)
	assert.Equal(t, "cur_1", seen.query.Get("after"))
	assert.Equal(t, "clt_1,clt_2", seen.query.Get("clientIds"))
	assert.Equal(t, "app_1", seen.query.Get("applicationIds"))
	assert.Equal(t, "prn_9", seen.query.Get("principalId"))
	assert.Equal(t, "prn_1", seen.query.Get("entityId"))
	assert.Empty(t, seen.query.Get("clientId"))
	assert.True(t, out.HasMore)
	assert.Equal(t, "cur_2", out.NextCursor)
	require.Len(t, out.AuditLogs, 1)
	assert.Equal(t, "{}", out.AuditLogs[0].OperationJSON)
}

// ─── Scheduled jobs ──────────────────────────────────────────────────

func TestScheduledJobsNoContentOperations(t *testing.T) {
	srv, seen := newStatusSrv(t, http.StatusNoContent, ``)
	c := client.New(srv.URL)
	ctx := context.Background()
	sj := c.ScheduledJobs()

	name := "Nightly"
	require.NoError(t, sj.Update(ctx, "sjb_1", &client.UpdateScheduledJobRequest{Name: &name}))
	assert.Equal(t, http.MethodPut, seen.method)
	assert.Equal(t, "/api/scheduled-jobs/sjb_1", seen.path)

	for verb, call := range map[string]func(context.Context, string) error{
		"pause": sj.Pause, "resume": sj.Resume, "archive": sj.Archive,
	} {
		require.NoError(t, call(ctx, "sjb_1"))
		assert.Equal(t, http.MethodPost, seen.method)
		assert.Equal(t, "/api/scheduled-jobs/sjb_1/"+verb, seen.path)
	}

	require.NoError(t, sj.CompleteInstance(ctx, "sji_1", &client.InstanceCompleteRequest{Status: client.CompletionStatusSuccess, Result: json.RawMessage(`{"n":1}`)}))
	assert.Equal(t, http.MethodPost, seen.method)
	assert.Equal(t, "/api/scheduled-jobs/instances/sji_1/complete", seen.path)
	assert.JSONEq(t, `{"status":"SUCCESS","result":{"n":1}}`, seen.body)
}

func TestScheduledJobsFireDecodesGoResponse(t *testing.T) {
	srv, seen := newStatusSrv(t, http.StatusAccepted, `{"id":"sji_1","scheduledJobId":"sjb_1","instanceId":"sji_1"}`)
	c := client.New(srv.URL)

	out, err := c.ScheduledJobs().Fire(context.Background(), "sjb_1", &client.FireRequest{CorrelationID: "corr"})
	require.NoError(t, err)
	assert.Equal(t, "/api/scheduled-jobs/sjb_1/fire", seen.path)
	assert.Equal(t, "sji_1", out.ID)
	assert.Equal(t, "sjb_1", out.ScheduledJobID)
	assert.Equal(t, "sji_1", out.InstanceID)
}

func TestScheduledJobsListInstancesSendsGoFilters(t *testing.T) {
	srv, seen := newMockSrv(t, `{"data":[{"id":"sji_1","scheduledJobId":"sjb_1","jobCode":"nightly","triggerKind":"CRON","firedAt":"","status":"COMPLETED","deliveryAttempts":1,"createdAt":""}],"page":0,"size":20,"total":41,"total_pages":3}`)
	c := client.New(srv.URL)

	size := uint32(20)
	out, err := c.ScheduledJobs().ListInstances(context.Background(), "sjb_1", &client.InstanceFilters{Status: "COMPLETED", Size: &size})
	require.NoError(t, err)
	assert.Equal(t, "/api/scheduled-jobs/sjb_1/instances", seen.path)
	assert.Equal(t, "COMPLETED", seen.query.Get("status"))
	assert.Equal(t, "20", seen.query.Get("size"))
	assert.Len(t, seen.query, 2)
	assert.Equal(t, uint32(3), out.TotalPages)
	require.Len(t, out.Data, 1)
}

// ─── Router ──────────────────────────────────────────────────────────

func TestRouterInPipelineReadsTopLevelFields(t *testing.T) {
	srv, _ := newMockSrv(t, `{"messageId":"msg_1","inPipeline":true,"poolCode":"default","queueId":"q-main"}`)
	c := client.New(srv.URL)

	out, err := c.Router().InPipeline(context.Background(), "msg_1")
	require.NoError(t, err)
	assert.True(t, out.InPipeline)
	assert.Equal(t, "default", out.PoolCode)
	assert.Equal(t, "q-main", out.QueueID)
}

func TestRouterInPipelineLiftsLegacyDetail(t *testing.T) {
	srv, _ := newMockSrv(t, `{"messageId":"msg_1","inPipeline":true,"detail":{"messageId":"msg_1","queueId":"q-old","poolCode":"legacy","elapsedTimeMs":5,"addedToInPipelineAt":""}}`)
	c := client.New(srv.URL)

	out, err := c.Router().InPipeline(context.Background(), "msg_1")
	require.NoError(t, err)
	assert.Equal(t, "legacy", out.PoolCode)
	assert.Equal(t, "q-old", out.QueueID)
}

// ─── Subscriptions ───────────────────────────────────────────────────

func TestSubscriptionsCreateReturnsIDAndUpdateIsNoContent(t *testing.T) {
	srv, seen := newStatusSrv(t, http.StatusCreated, `{"id":"sub_new"}`)
	c := client.New(srv.URL)

	out, err := c.Subscriptions().Create(context.Background(), &client.CreateSubscriptionRequest{Code: "s", Name: "S", Endpoint: "https://example.com"})
	require.NoError(t, err)
	assert.Equal(t, "/api/subscriptions", seen.path)
	assert.Equal(t, "sub_new", out.ID)

	srv2, seen2 := newStatusSrv(t, http.StatusNoContent, ``)
	c2 := client.New(srv2.URL)
	name := "S2"
	require.NoError(t, c2.Subscriptions().Update(context.Background(), "sub_1", &client.UpdateSubscriptionRequest{Name: &name}))
	assert.Equal(t, http.MethodPut, seen2.method)
	assert.Equal(t, "/api/subscriptions/sub_1", seen2.path)
}

// ─── Clients ─────────────────────────────────────────────────────────

func TestClientsListTakesNoFiltersAndDecodesNotes(t *testing.T) {
	srv, seen := newMockSrv(t, `{"clients":[{"id":"clt_1","name":"Acme","identifier":"acme","status":"ACTIVE","notes":[{"category":"ops","text":"hi","addedBy":"prn_1","addedAt":"2026-01-01T00:00:00Z"}],"createdAt":"","updatedAt":""}],"total":1}`)
	c := client.New(srv.URL)

	out, err := c.Clients().List(context.Background())
	require.NoError(t, err)
	assert.Equal(t, "/api/clients", seen.path)
	assert.Empty(t, seen.query)
	require.Len(t, out.Clients, 1)
	require.Len(t, out.Clients[0].Notes, 1)
	assert.Equal(t, "hi", out.Clients[0].Notes[0].Text)
}

func TestClientsNoContentOperations(t *testing.T) {
	srv, seen := newStatusSrv(t, http.StatusNoContent, ``)
	c := client.New(srv.URL)
	ctx := context.Background()

	name := "Acme 2"
	require.NoError(t, c.Clients().Update(ctx, "clt_1", &client.UpdateClientRequest{Name: &name}))
	assert.Equal(t, http.MethodPut, seen.method)
	assert.Equal(t, "/api/clients/clt_1", seen.path)

	require.NoError(t, c.Clients().EnableApplication(ctx, "clt_1", "app_1"))
	assert.Equal(t, "/api/clients/clt_1/applications/app_1/enable", seen.path)

	require.NoError(t, c.Clients().DisableApplication(ctx, "clt_1", "app_1"))
	assert.Equal(t, "/api/clients/clt_1/applications/app_1/disable", seen.path)

	require.NoError(t, c.Clients().UpdateApplications(ctx, "clt_1", &client.UpdateClientApplicationsRequest{EnabledApplicationIDs: []string{"app_1"}}))
	assert.Equal(t, http.MethodPut, seen.method)
	assert.Equal(t, "/api/clients/clt_1/applications", seen.path)
	assert.JSONEq(t, `{"enabledApplicationIds":["app_1"]}`, seen.body)
}

// ─── Roles ───────────────────────────────────────────────────────────

func TestRolesUpdateSendsPermissionsOnlyWhenSet(t *testing.T) {
	srv, seen := newStatusSrv(t, http.StatusNoContent, ``)
	c := client.New(srv.URL)

	display := "Admin"
	require.NoError(t, c.Roles().Update(context.Background(), "orders:admin", &client.UpdateRoleRequest{DisplayName: &display}))
	assert.JSONEq(t, `{"displayName":"Admin"}`, seen.body)

	perms := []string{"orders:read"}
	require.NoError(t, c.Roles().Update(context.Background(), "orders:admin", &client.UpdateRoleRequest{Permissions: &perms}))
	assert.Equal(t, http.MethodPut, seen.method)
	assert.Equal(t, "/api/roles/orders:admin", seen.path)
	assert.JSONEq(t, `{"permissions":["orders:read"]}`, seen.body)
}

// Go's AddSchemaRequest requires `version`: it is always sent.
func TestEventTypesAddSchemaVersionAlwaysSendsVersion(t *testing.T) {
	srv, seen := newStatusSrv(t, http.StatusOK, `{"id":"et_1","code":"a:b:c:d","name":"D"}`)
	c := client.New(srv.URL)

	_, err := c.EventTypes().AddSchemaVersion(context.Background(), "et_1",
		&client.AddSchemaVersionRequest{Schema: json.RawMessage(`{"type":"object"}`)})
	require.NoError(t, err)
	assert.Equal(t, "POST", seen.method)
	assert.Equal(t, "/api/event-types/et_1/versions", seen.path)
	var body map[string]any
	require.NoError(t, json.Unmarshal([]byte(seen.body), &body))
	_, has := body["version"]
	assert.True(t, has, "version is required by Go and must be sent: %s", seen.body)
}
