package auth_test

import (
	"encoding/json"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/flowcatalyst/flowcatalyst/clients/go-sdk/auth"
)

func makeClaims(scope, principalType string, clients, roles []string) auth.AccessTokenClaims {
	return auth.AccessTokenClaims{
		Sub:           "prn_test123",
		Iss:           "flowcatalyst",
		Aud:           "flowcatalyst",
		Exp:           9999999999,
		Iat:           1000000000,
		Nbf:           1000000000,
		Jti:           "jti_abc",
		PrincipalType: principalType,
		Scope:         scope,
		Email:         "user@example.com",
		Name:          "Test User",
		Clients:       clients,
		Roles:         roles,
	}
}

func TestClaimsHasClientAccessWildcardAndSpecific(t *testing.T) {
	wc := makeClaims("ANCHOR", "USER", []string{"*"}, nil)
	assert.True(t, wc.HasClientAccess("clt_anything"))

	scoped := makeClaims("CLIENT", "USER", []string{"clt_a", "clt_b"}, nil)
	assert.True(t, scoped.HasClientAccess("clt_a"))
	assert.False(t, scoped.HasClientAccess("clt_c"))
}

func TestClaimsScopeFlags(t *testing.T) {
	anchor := makeClaims("ANCHOR", "USER", []string{"*"}, nil)
	assert.True(t, anchor.IsAnchor())
	assert.False(t, anchor.IsService())

	svc := makeClaims("CLIENT", "SERVICE", []string{"clt_1"}, nil)
	assert.True(t, svc.IsService())
	assert.False(t, svc.IsAnchor())
}

func TestClaimsHasRole(t *testing.T) {
	c := makeClaims("CLIENT", "USER", []string{"clt_1"}, []string{"admin", "editor"})
	assert.True(t, c.HasRole("admin"))
	assert.True(t, c.HasRole("editor"))
	assert.False(t, c.HasRole("viewer"))
}

func TestClaimsSerdeRoundTripPreservesTypeRename(t *testing.T) {
	c := makeClaims("ANCHOR", "SERVICE", []string{"*"}, []string{"admin"})
	raw, err := json.Marshal(c)
	require.NoError(t, err)
	assert.Contains(t, string(raw), `"type":"SERVICE"`)
	assert.NotContains(t, string(raw), `"principal_type":`)

	var back auth.AccessTokenClaims
	require.NoError(t, json.Unmarshal(raw, &back))
	assert.Equal(t, "SERVICE", back.PrincipalType)
}

func TestClaimsDeserializesWithoutEmail(t *testing.T) {
	raw := `{
		"sub":"prn_1","iss":"fc","aud":"fc","exp":9999999999,"iat":0,"nbf":0,
		"jti":"j1","type":"SERVICE","scope":"CLIENT","name":"svc","clients":["clt_1"]
	}`
	var c auth.AccessTokenClaims
	require.NoError(t, json.Unmarshal([]byte(raw), &c))
	assert.Empty(t, c.Email)
	assert.Equal(t, "SERVICE", c.PrincipalType)
}

func TestAuthContextDelegatesToClaims(t *testing.T) {
	c := makeClaims("ANCHOR", "USER", []string{"*"}, []string{"admin"})
	ctx := auth.NewAuthContext(c, "eyJtoken")

	assert.Equal(t, "prn_test123", ctx.PrincipalID())
	assert.Equal(t, "user@example.com", ctx.Email())
	assert.Equal(t, "Test User", ctx.Name())
	assert.True(t, ctx.IsAnchor())
	assert.True(t, ctx.HasClientAccess("clt_anything"))
	assert.True(t, ctx.HasRole("admin"))
	assert.Equal(t, []string{"*"}, ctx.ClientIDs())
	assert.Equal(t, []string{"admin"}, ctx.Roles())
	assert.Equal(t, "eyJtoken", ctx.BearerToken())
}

// ─── Go's claim shape ───────────────────────────────────────────────────

func goAPIToken(t *testing.T) auth.AccessTokenClaims {
	t.Helper()
	var c auth.AccessTokenClaims
	require.NoError(t, json.Unmarshal([]byte(`{
		"iss": "https://fc.example.com", "sub": "prn_1", "aud": "flowcatalyst",
		"exp": 9999999999, "iat": 1000000000, "nbf": 1000000000, "jti": "j1",
		"type": "USER", "tier": "CLIENT",
		"scope": "orders:order:read  orders:order:write",
		"email": "u@example.com", "name": "U",
		"clients": ["clt_a:acme", "clt_b"],
		"roles": ["orders:viewer"],
		"applications": ["app_1:orders", "app_2"],
		"all_applications": false,
		"token_use": "api"
	}`), &c))
	return c
}

func TestGoShapeTierAndScopeAreReadApart(t *testing.T) {
	c := goAPIToken(t)
	assert.Equal(t, "CLIENT", c.TenancyTier())
	assert.False(t, c.IsAnchor())
	assert.Equal(t, []string{"orders:order:read", "orders:order:write"}, c.GrantedPermissions())
	assert.False(t, c.IsIdentityToken())
}

func TestGoShapeClientAndApplicationPairsMatchByID(t *testing.T) {
	c := goAPIToken(t)
	assert.True(t, c.HasClientAccess("clt_a"))
	assert.True(t, c.HasClientAccess("clt_b"))
	assert.False(t, c.HasClientAccess("acme"))
	assert.False(t, c.HasClientAccess("clt_c"))
	assert.Equal(t, []string{"clt_a", "clt_b"}, c.ClientIDList())
	assert.True(t, c.HasApplicationAccess("app_1"))
	assert.True(t, c.HasApplicationAccess("app_2"))
	assert.False(t, c.HasApplicationAccess("app_3"))
	assert.Equal(t, []string{"app_1", "app_2"}, c.ApplicationIDs())
	assert.False(t, c.HasAllApplications())
}

func TestGoShapeAnchorAndAllApplications(t *testing.T) {
	var c auth.AccessTokenClaims
	require.NoError(t, json.Unmarshal([]byte(`{
		"sub": "prn_1", "type": "USER", "tier": "ANCHOR", "name": "A",
		"clients": ["*"], "roles": [], "applications": ["*"], "all_applications": true
	}`), &c))
	assert.True(t, c.IsAnchor())
	assert.Empty(t, c.GrantedPermissions())
	assert.True(t, c.HasAllApplications())
	assert.True(t, c.HasApplicationAccess("app_anything"))
	assert.Empty(t, c.ApplicationIDs())
}

func TestGoShapeIdentityTokenCarriesNoAuthority(t *testing.T) {
	var c auth.AccessTokenClaims
	require.NoError(t, json.Unmarshal([]byte(`{
		"sub": "prn_1", "type": "USER", "tier": "PARTNER", "name": "P",
		"clients": [], "roles": [], "applications": [], "all_applications": false,
		"token_use": "identity"
	}`), &c))
	assert.True(t, c.IsIdentityToken())
	assert.Equal(t, "PARTNER", c.TenancyTier())
	assert.Empty(t, c.GrantedPermissions())
}

func TestLegacyScopeAsTierTokenStillReadsItsTier(t *testing.T) {
	var c auth.AccessTokenClaims
	require.NoError(t, json.Unmarshal([]byte(`{
		"sub": "prn_1", "type": "USER", "scope": "ANCHOR", "name": "L", "clients": ["*"]
	}`), &c))
	assert.Equal(t, "ANCHOR", c.TenancyTier())
	assert.True(t, c.IsAnchor())
	assert.Empty(t, c.GrantedPermissions())
}
