package client_test

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"net/url"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/flowcatalyst/flowcatalyst/clients/go-sdk/client"
)

// newMockSrv records the last seen request and returns the body the
// caller supplies. URL = srv.URL.
func newMockSrv(t *testing.T, respJSON string) (*httptest.Server, *seenRequest) {
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
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(respJSON))
	}))
	t.Cleanup(srv.Close)
	return srv, seen
}

type seenRequest struct {
	method string
	path   string
	query  url.Values
	body   string
}

func TestAuditLogsListBuildsQuery(t *testing.T) {
	srv, seen := newMockSrv(t, `{"auditLogs":[],"total":0,"page":1,"pageSize":50}`)
	c := client.New(srv.URL)

	page := uint32(2)
	pageSize := uint32(25)
	_, err := c.AuditLogs().List(context.Background(), &client.AuditLogFilters{
		EntityType: "Principal",
		Operation:  "ROLE_ASSIGNED",
		Page:       &page,
		PageSize:   &pageSize,
	})
	require.NoError(t, err)
	assert.Equal(t, http.MethodGet, seen.method)
	assert.Equal(t, "/api/audit-logs", seen.path)
	assert.Equal(t, "Principal", seen.query.Get("entityType"))
	assert.Equal(t, "ROLE_ASSIGNED", seen.query.Get("operation"))
	assert.Equal(t, "2", seen.query.Get("page"))
	assert.Equal(t, "25", seen.query.Get("pageSize"))
}

func TestAuditLogsGetDeserializes(t *testing.T) {
	srv, _ := newMockSrv(t, `{"id":"aud_1","operation":"CREATE","entityType":"Principal","entityId":"prn_1","performedAt":"2026-01-01T00:00:00Z"}`)
	c := client.New(srv.URL)

	r, err := c.AuditLogs().Get(context.Background(), "aud_1")
	require.NoError(t, err)
	assert.Equal(t, "aud_1", r.ID)
	assert.Equal(t, "CREATE", r.Operation)
	assert.Equal(t, "Principal", r.EntityType)
}

func TestPermissionsListDeserializes(t *testing.T) {
	srv, seen := newMockSrv(t, `{"permissions":[{"permission":"orders:read","application":"orders","context":"client","aggregate":"Order","action":"read","description":"Read orders"}],"total":1}`)
	c := client.New(srv.URL)

	r, err := c.Permissions().List(context.Background())
	require.NoError(t, err)
	assert.Equal(t, "/api/roles/permissions", seen.path)
	require.Len(t, r.Permissions, 1)
	assert.Equal(t, "orders:read", r.Permissions[0].Permission)
}

func TestMeClientApplicationsBuildsPath(t *testing.T) {
	srv, seen := newMockSrv(t, `{"applications":[],"clientId":"clt_1"}`)
	c := client.New(srv.URL)

	_, err := c.Me().ClientApplications(context.Background(), "clt_1")
	require.NoError(t, err)
	assert.Equal(t, "/api/me/clients/clt_1/applications", seen.path)
}

func TestClientsCreateSendsBody(t *testing.T) {
	srv, seen := newMockSrv(t, `{"id":"clt_new"}`)
	c := client.New(srv.URL)

	r, err := c.Clients().Create(context.Background(), &client.CreateClientRequest{
		Identifier: "acme", Name: "Acme",
	})
	require.NoError(t, err)
	assert.Equal(t, "clt_new", r.ID)
	assert.Equal(t, http.MethodPost, seen.method)
	var body client.CreateClientRequest
	require.NoError(t, json.Unmarshal([]byte(seen.body), &body))
	assert.Equal(t, "acme", body.Identifier)
}

func TestClientsSearchEncodesQ(t *testing.T) {
	srv, seen := newMockSrv(t, `{"clients":[]}`)
	c := client.New(srv.URL)

	_, err := c.Clients().Search(context.Background(), "ac me")
	require.NoError(t, err)
	assert.Equal(t, "/api/clients/search", seen.path)
	assert.Equal(t, "ac me", seen.query.Get("q"))
}

func TestConnectionsListEncodesFilters(t *testing.T) {
	srv, seen := newMockSrv(t, `{"connections":[]}`)
	c := client.New(srv.URL)

	_, err := c.Connections().List(context.Background(), "clt_1", "ACTIVE", "")
	require.NoError(t, err)
	assert.Equal(t, "/api/connections", seen.path)
	assert.Equal(t, "clt_1", seen.query.Get("clientId"))
	assert.Equal(t, "ACTIVE", seen.query.Get("status"))
	assert.Empty(t, seen.query.Get("serviceAccountId"))
}

func TestConnectionsPauseIsPost(t *testing.T) {
	srv, seen := newMockSrv(t, ``)
	c := client.New(srv.URL)

	require.NoError(t, c.Connections().Pause(context.Background(), "con_1"))
	assert.Equal(t, http.MethodPost, seen.method)
	assert.Equal(t, "/api/connections/con_1/pause", seen.path)
}

func TestRouterUsesRouterBaseURL(t *testing.T) {
	apiSrv, _ := newMockSrv(t, `{"messageId":"msg_1","inPipeline":false}`)
	var routerSeen *seenRequest
	routerSrv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		routerSeen = &seenRequest{method: r.Method, path: r.URL.Path, query: r.URL.Query()}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"messageId":"msg_1","inPipeline":false}`))
	}))
	defer routerSrv.Close()

	c := client.New(apiSrv.URL, client.WithRouterBaseURL(routerSrv.URL))
	_, err := c.Router().InPipeline(context.Background(), "msg_1")
	require.NoError(t, err)
	require.NotNil(t, routerSeen)
	assert.Equal(t, http.MethodGet, routerSeen.method)
	assert.Equal(t, "/monitoring/in-flight-messages/check", routerSeen.path)
	assert.Equal(t, "msg_1", routerSeen.query.Get("messageId"))
}

func TestRouterInPipelineBatch(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		assert.Equal(t, http.MethodPost, r.Method)
		assert.Equal(t, "/monitoring/in-flight-messages/check-batch", r.URL.Path)
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"msg_1":true,"msg_2":false}`))
	}))
	defer srv.Close()
	c := client.New(srv.URL) // single URL: router falls back to base

	out, err := c.Router().InPipelineBatch(context.Background(), []string{"msg_1", "msg_2"})
	require.NoError(t, err)
	assert.True(t, out["msg_1"])
	assert.False(t, out["msg_2"])
}

func TestScheduledJobsCreateAndFire(t *testing.T) {
	srv, seen := newMockSrv(t, `{"id":"sjb_1"}`)
	c := client.New(srv.URL)

	_, err := c.ScheduledJobs().Create(context.Background(), &client.CreateScheduledJobRequest{
		Code: "daily-sync", Name: "Daily Sync", Crons: []string{"0 0 * * *"},
	})
	require.NoError(t, err)
	assert.Equal(t, "/api/scheduled-jobs", seen.path)
	var body client.CreateScheduledJobRequest
	require.NoError(t, json.Unmarshal([]byte(seen.body), &body))
	assert.Equal(t, []string{"0 0 * * *"}, body.Crons)
}

func TestScheduledJobsListPaginatedShape(t *testing.T) {
	srv, _ := newMockSrv(t, `{"data":[{"id":"sjb_1","code":"x","name":"X","status":"ACTIVE","crons":["0 * * * *"],"timezone":"UTC","concurrent":false,"tracksCompletion":false,"deliveryMaxAttempts":3,"createdAt":"","updatedAt":"","version":1}],"page":1,"size":50,"total":1,"totalPages":1}`)
	c := client.New(srv.URL)

	r, err := c.ScheduledJobs().List(context.Background(), nil)
	require.NoError(t, err)
	assert.Equal(t, uint32(1), r.Page)
	require.Len(t, r.Data, 1)
	assert.Equal(t, "sjb_1", r.Data[0].ID)
}

func TestOpenAPISyncPostsSpec(t *testing.T) {
	srv, seen := newMockSrv(t, `{"applicationCode":"orders","specId":"sp_1","version":"1.0","status":"PUBLISHED","hasBreaking":false,"unchanged":false}`)
	c := client.New(srv.URL)

	spec := json.RawMessage(`{"openapi":"3.0.0","info":{"title":"Orders","version":"1.0.0"},"paths":{}}`)
	r, err := c.OpenAPI().Sync(context.Background(), "orders", spec)
	require.NoError(t, err)
	assert.Equal(t, "/api/applications/orders/openapi/sync", seen.path)
	assert.Equal(t, "PUBLISHED", r.Status)

	var body client.SyncOpenAPISpecRequest
	require.NoError(t, json.Unmarshal([]byte(seen.body), &body))
	assert.Contains(t, string(body.Spec), `"openapi":"3.0.0"`)
}

// ─── B: deferred methods on existing resources ───────────────────────

func TestApplicationsProvisionServiceAccount(t *testing.T) {
	srv, seen := newMockSrv(t, `{"id":"prn_svc","code":"svc","name":"Svc","active":true,"applicationId":"app_1","createdAt":""}`)
	c := client.New(srv.URL)

	r, err := c.Applications().ProvisionServiceAccount(context.Background(), "app_1")
	require.NoError(t, err)
	assert.Equal(t, "/api/applications/app_1/provision-service-account", seen.path)
	assert.Equal(t, http.MethodPost, seen.method)
	assert.Equal(t, "prn_svc", r.ID)
}

func TestApplicationsListRolesUsesByID(t *testing.T) {
	srv, seen := newMockSrv(t, `[{"id":"rol_1","code":"orders:admin","displayName":"Admin","applicationCode":"orders","source":"PLATFORM"}]`)
	c := client.New(srv.URL)

	r, err := c.Applications().ListRoles(context.Background(), "app_1")
	require.NoError(t, err)
	assert.Equal(t, "/api/applications/by-id/app_1/roles", seen.path)
	require.Len(t, r, 1)
	assert.Equal(t, "orders:admin", r[0].Code)
}

func TestApplicationsEnableForClient(t *testing.T) {
	srv, seen := newMockSrv(t, `{"id":"cfg_1","applicationId":"app_1","clientId":"clt_1","enabled":true}`)
	c := client.New(srv.URL)

	r, err := c.Applications().EnableForClient(context.Background(), "app_1", "clt_1")
	require.NoError(t, err)
	assert.Equal(t, "/api/applications/app_1/clients/clt_1/enable", seen.path)
	assert.True(t, r.Enabled)
}

func TestPrincipalsAddRoleSendsRoleField(t *testing.T) {
	srv, seen := newMockSrv(t, ``)
	c := client.New(srv.URL)

	require.NoError(t, c.Principals().AddRole(context.Background(), "prn_1", "orders:admin"))
	assert.Equal(t, "/api/principals/prn_1/roles", seen.path)
	assert.Equal(t, http.MethodPost, seen.method)
	var body client.AssignRoleRequest
	require.NoError(t, json.Unmarshal([]byte(seen.body), &body))
	assert.Equal(t, "orders:admin", body.Role)
}

func TestPrincipalsSetRolesReplaces(t *testing.T) {
	srv, seen := newMockSrv(t, ``)
	c := client.New(srv.URL)

	require.NoError(t, c.Principals().SetRoles(context.Background(), "prn_1", []string{"orders:admin", "orders:viewer"}))
	assert.Equal(t, http.MethodPut, seen.method)
	assert.Equal(t, "/api/principals/prn_1/roles", seen.path)
	var body client.ReplaceRolesRequest
	require.NoError(t, json.Unmarshal([]byte(seen.body), &body))
	assert.Equal(t, []string{"orders:admin", "orders:viewer"}, body.Roles)
}

func TestPrincipalsGrantAndRevokeClientAccess(t *testing.T) {
	srv, seen := newMockSrv(t, ``)
	c := client.New(srv.URL)

	require.NoError(t, c.Principals().GrantClientAccess(context.Background(), "prn_1", "clt_x"))
	assert.Equal(t, http.MethodPost, seen.method)
	assert.Equal(t, "/api/principals/prn_1/client-access", seen.path)
	var body client.GrantClientAccessRequest
	require.NoError(t, json.Unmarshal([]byte(seen.body), &body))
	assert.Equal(t, "clt_x", body.ClientID)

	require.NoError(t, c.Principals().RevokeClientAccess(context.Background(), "prn_1", "clt_x"))
	assert.Equal(t, http.MethodDelete, seen.method)
	assert.Equal(t, "/api/principals/prn_1/client-access/clt_x", seen.path)
}

func TestRolesGrantAndRevokePermission(t *testing.T) {
	srv, seen := newMockSrv(t, `{"id":"rol_1","name":"orders:admin","shortName":"admin","displayName":"Admin","applicationCode":"orders","source":"PLATFORM","permissions":["orders:read"],"createdAt":"","updatedAt":""}`)
	c := client.New(srv.URL)

	r, err := c.Roles().GrantPermission(context.Background(), "orders:admin", "orders:read")
	require.NoError(t, err)
	assert.Equal(t, http.MethodPost, seen.method)
	assert.Equal(t, "/api/roles/orders:admin/permissions", seen.path)
	assert.Equal(t, []string{"orders:read"}, r.Permissions)

	_, err = c.Roles().RevokePermission(context.Background(), "orders:admin", "orders:read")
	require.NoError(t, err)
	assert.Equal(t, http.MethodDelete, seen.method)
	assert.Equal(t, "/api/roles/orders:admin/permissions/orders:read", seen.path)
}

func TestRolesListForApplication(t *testing.T) {
	// The platform answers a bare array.
	srv, seen := newMockSrv(t, `[{"id":"rol_1","name":"orders:admin","shortName":"admin","displayName":"Admin","applicationCode":"orders","source":"PLATFORM","createdAt":"","updatedAt":""}]`)
	c := client.New(srv.URL)

	r, err := c.Roles().ListForApplication(context.Background(), "app_1")
	require.NoError(t, err)
	assert.Equal(t, "/api/roles/by-application/app_1", seen.path)
	require.Len(t, r.Roles, 1)
	assert.Equal(t, uint64(1), r.Total)
	assert.Equal(t, "orders:admin", r.Roles[0].Name)
}

func TestRolesCreateAlwaysSendsClientManaged(t *testing.T) {
	srv, seen := newMockSrv(t, `{"id":"rol_1"}`)
	c := client.New(srv.URL)

	_, err := c.Roles().Create(context.Background(), &client.CreateRoleRequest{
		ApplicationCode: "orders", RoleName: "viewer", DisplayName: "Viewer",
	})
	require.NoError(t, err)
	assert.Contains(t, seen.body, `"clientManaged":false`)
}

func TestScheduledJobsCreateAlwaysSendsRequiredFlags(t *testing.T) {
	srv, seen := newMockSrv(t, `{"id":"sjb_1"}`)
	c := client.New(srv.URL)

	_, err := c.ScheduledJobs().Create(context.Background(), &client.CreateScheduledJobRequest{
		Code: "nightly", Name: "Nightly", Crons: []string{"0 0 2 * * *"},
	})
	require.NoError(t, err)
	assert.Contains(t, seen.body, `"concurrent":false`)
	assert.Contains(t, seen.body, `"tracksCompletion":false`)
}

func TestScheduledJobsListInstanceLogsReadsBareArray(t *testing.T) {
	srv, seen := newMockSrv(t, `[{"id":"lg_1","instanceId":"sji_1","level":"INFO","message":"hi","createdAt":""}]`)
	c := client.New(srv.URL)

	out, err := c.ScheduledJobs().ListInstanceLogs(context.Background(), "sji_1")
	require.NoError(t, err)
	assert.Equal(t, "/api/scheduled-jobs/instances/sji_1/logs", seen.path)
	require.Len(t, out.Logs, 1)
	assert.Equal(t, "hi", out.Logs[0].Message)
	assert.Equal(t, uint64(1), out.Total)
}

func TestScheduledJobsLogForInstanceDefaultsLevelToInfo(t *testing.T) {
	srv, seen := newMockSrv(t, `{"id":"lg_1","instanceId":"sji_1","level":"INFO","message":"m","createdAt":""}`)
	c := client.New(srv.URL)

	req := &client.InstanceLogRequest{Message: "m"}
	_, err := c.ScheduledJobs().LogForInstance(context.Background(), "sji_1", req)
	require.NoError(t, err)
	assert.Contains(t, seen.body, `"level":"INFO"`)
	assert.Equal(t, client.LogLevel(""), req.Level, "the caller's request is not mutated")
}

// Owner ruling 2 of 2026-09-25 (Java 714f3f2d): the router will verify the
// platform bearer token, so both in-flight checks must carry the one the
// client uses for the platform.
func TestRouterCallsSendThePlatformBearerToken(t *testing.T) {
	var seen []string
	routerSrv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		seen = append(seen, r.Header.Get("Authorization"))
		w.Header().Set("Content-Type", "application/json")
		if r.URL.Path == "/monitoring/in-flight-messages/check-batch" {
			_, _ = w.Write([]byte(`{"m1":true}`))
			return
		}
		_, _ = w.Write([]byte(`{"messageId":"m1","inPipeline":false}`))
	}))
	defer routerSrv.Close()

	c := client.New("http://127.0.0.1:1", client.WithRouterBaseURL(routerSrv.URL), client.WithToken("tok-1"))
	_, err := c.Router().InPipeline(context.Background(), "m1")
	require.NoError(t, err)
	_, err = c.Router().InPipelineBatch(context.Background(), []string{"m1"})
	require.NoError(t, err)
	assert.Equal(t, []string{"Bearer tok-1", "Bearer tok-1"}, seen)
}
