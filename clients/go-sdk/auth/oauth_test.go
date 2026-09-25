package auth_test

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/flowcatalyst/flowcatalyst/clients/go-sdk/auth"
)

func TestAuthorizeURLContainsRequiredQueryParams(t *testing.T) {
	c := auth.NewOAuthClient(auth.OAuthConfig{
		IssuerURL:   "https://auth.example.com/",
		ClientID:    "my-app",
		RedirectURI: "https://app.example.com/callback",
		Scopes:      []string{"openid", "profile"},
	})

	urlStr, params := c.AuthorizeURL()
	assert.True(t, strings.HasPrefix(urlStr, "https://auth.example.com/oauth/authorize?"))
	assert.NotContains(t, urlStr, "//oauth") // trailing slash stripped

	u, err := url.Parse(urlStr)
	require.NoError(t, err)
	q := u.Query()
	assert.Equal(t, "code", q.Get("response_type"))
	assert.Equal(t, "my-app", q.Get("client_id"))
	assert.Equal(t, "https://app.example.com/callback", q.Get("redirect_uri"))
	assert.Equal(t, "openid profile", q.Get("scope"))
	assert.Equal(t, "S256", q.Get("code_challenge_method"))
	assert.Equal(t, params.PKCE.CodeChallenge, q.Get("code_challenge"))
	assert.Equal(t, params.State, q.Get("state"))
	assert.Equal(t, params.Nonce, q.Get("nonce"))
}

func TestAuthorizeURLValuesAreUniquePerCall(t *testing.T) {
	c := auth.NewOAuthClient(auth.OAuthConfig{
		IssuerURL: "https://auth.example.com", ClientID: "app", RedirectURI: "https://cb",
	})
	_, a := c.AuthorizeURL()
	_, b := c.AuthorizeURL()
	assert.NotEqual(t, a.State, b.State)
	assert.NotEqual(t, a.Nonce, b.Nonce)
	assert.NotEqual(t, a.PKCE.CodeVerifier, b.PKCE.CodeVerifier)
}

func newOAuthMockServer(t *testing.T) (*httptest.Server, *struct {
	tokenForm  url.Values
	revokeForm url.Values
}) {
	state := &struct {
		tokenForm  url.Values
		revokeForm url.Values
	}{}
	mux := http.NewServeMux()
	mux.HandleFunc("/oauth/token", func(w http.ResponseWriter, r *http.Request) {
		require.NoError(t, r.ParseForm())
		state.tokenForm = r.PostForm
		_ = json.NewEncoder(w).Encode(map[string]any{
			"access_token":  "at_abc",
			"token_type":    "Bearer",
			"expires_in":    3600,
			"refresh_token": "rt_def",
			"id_token":      "id_xyz",
		})
	})
	mux.HandleFunc("/oauth/revoke", func(w http.ResponseWriter, r *http.Request) {
		require.NoError(t, r.ParseForm())
		state.revokeForm = r.PostForm
		w.WriteHeader(http.StatusOK)
	})
	mux.HandleFunc("/oauth/introspect", func(w http.ResponseWriter, _ *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{
			"active": true, "sub": "prn_42", "client_id": "app",
		})
	})
	mux.HandleFunc("/oauth/userinfo", func(w http.ResponseWriter, r *http.Request) {
		assert.Equal(t, "Bearer at_abc", r.Header.Get("Authorization"))
		_ = json.NewEncoder(w).Encode(map[string]any{
			"sub": "prn_42", "email": "u@example.com", "email_verified": true,
			"custom": "value", "org_id": 99,
		})
	})
	srv := httptest.NewServer(mux)
	t.Cleanup(srv.Close)
	return srv, state
}

func TestOAuthExchangeCodeSendsExpectedForm(t *testing.T) {
	srv, state := newOAuthMockServer(t)
	c := auth.NewOAuthClient(auth.OAuthConfig{
		IssuerURL:    srv.URL,
		ClientID:     "my-app",
		ClientSecret: "shh",
		RedirectURI:  "https://app.example.com/cb",
	})

	tokens, err := c.ExchangeCode(context.Background(), "code123", "ver456")
	require.NoError(t, err)
	assert.Equal(t, "at_abc", tokens.AccessToken)
	assert.Equal(t, "rt_def", tokens.RefreshToken)
	assert.Equal(t, "id_xyz", tokens.IDToken)
	assert.Equal(t, int64(3600), tokens.ExpiresIn)

	assert.Equal(t, "authorization_code", state.tokenForm.Get("grant_type"))
	assert.Equal(t, "code123", state.tokenForm.Get("code"))
	assert.Equal(t, "ver456", state.tokenForm.Get("code_verifier"))
	assert.Equal(t, "my-app", state.tokenForm.Get("client_id"))
	assert.Equal(t, "shh", state.tokenForm.Get("client_secret"))
}

func TestOAuthRefreshToken(t *testing.T) {
	srv, state := newOAuthMockServer(t)
	c := auth.NewOAuthClient(auth.OAuthConfig{
		IssuerURL: srv.URL, ClientID: "app", ClientSecret: "shh",
	})
	_, err := c.RefreshToken(context.Background(), "old_rt")
	require.NoError(t, err)
	assert.Equal(t, "refresh_token", state.tokenForm.Get("grant_type"))
	assert.Equal(t, "old_rt", state.tokenForm.Get("refresh_token"))
}

// Owner ruling 5 of 2026-09-25: concurrent refreshes of one token share one
// exchange, and a refresh just after it reuses its result, so the SDK never
// presents a rotated-out refresh token.
func TestOAuthRefreshTokenIsSingleFlight(t *testing.T) {
	var requests atomic.Int32
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		n := requests.Add(1)
		time.Sleep(50 * time.Millisecond)
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{
			"access_token":  "at-" + string(rune('0'+n)),
			"token_type":    "Bearer",
			"expires_in":    3600,
			"refresh_token": "rt-" + string(rune('0'+n)),
		})
	}))
	defer srv.Close()
	c := auth.NewOAuthClient(auth.OAuthConfig{IssuerURL: srv.URL, ClientID: "app", ClientSecret: "shh"})
	ctx := context.Background()

	var wg sync.WaitGroup
	results := make([]*auth.TokenResponse, 2)
	for i := range results {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			res, err := c.RefreshToken(ctx, "rt-old")
			require.NoError(t, err)
			results[i] = res
		}(i)
	}
	wg.Wait()
	assert.Equal(t, int32(1), requests.Load(), "joined the in-flight exchange")
	assert.Equal(t, results[0].AccessToken, results[1].AccessToken)

	again, err := c.RefreshToken(ctx, "rt-old")
	require.NoError(t, err)
	assert.Equal(t, int32(1), requests.Load(), "reused the result just after")
	assert.Equal(t, results[0].RefreshToken, again.RefreshToken)

	_, err = c.RefreshToken(ctx, "rt-other")
	require.NoError(t, err)
	assert.Equal(t, int32(2), requests.Load(), "a different refresh token is its own exchange")
}

func TestOAuthRefreshTokenFailureIsNotRemembered(t *testing.T) {
	var requests atomic.Int32
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requests.Add(1)
		http.Error(w, `{"error":"server_error"}`, http.StatusInternalServerError)
	}))
	defer srv.Close()
	c := auth.NewOAuthClient(auth.OAuthConfig{IssuerURL: srv.URL, ClientID: "app"})

	_, err := c.RefreshToken(context.Background(), "rt")
	require.Error(t, err)
	_, err = c.RefreshToken(context.Background(), "rt")
	require.Error(t, err)
	assert.Equal(t, int32(2), requests.Load(), "a failure is retried, not reused")
}

func TestOAuthRevokeToken(t *testing.T) {
	srv, state := newOAuthMockServer(t)
	c := auth.NewOAuthClient(auth.OAuthConfig{
		IssuerURL: srv.URL, ClientID: "app",
	})
	require.NoError(t, c.RevokeToken(context.Background(), "tok"))
	assert.Equal(t, "tok", state.revokeForm.Get("token"))
}

func TestOAuthIntrospect(t *testing.T) {
	srv, _ := newOAuthMockServer(t)
	c := auth.NewOAuthClient(auth.OAuthConfig{
		IssuerURL: srv.URL, ClientID: "app",
	})
	resp, err := c.IntrospectToken(context.Background(), "tok")
	require.NoError(t, err)
	assert.True(t, resp.Active)
	assert.Equal(t, "prn_42", resp.Sub)
}

func TestOAuthUserInfoExposesExtraClaims(t *testing.T) {
	srv, _ := newOAuthMockServer(t)
	c := auth.NewOAuthClient(auth.OAuthConfig{
		IssuerURL: srv.URL, ClientID: "app",
	})
	info, err := c.UserInfo(context.Background(), "at_abc")
	require.NoError(t, err)
	assert.Equal(t, "prn_42", info.Sub)
	assert.Equal(t, "u@example.com", info.Email)
	require.NotNil(t, info.EmailVerified)
	assert.True(t, *info.EmailVerified)
	assert.Equal(t, "value", info.Extra["custom"])
	assert.EqualValues(t, 99, info.Extra["org_id"])
}

func TestOAuthLogoutURL(t *testing.T) {
	c := auth.NewOAuthClient(auth.OAuthConfig{
		IssuerURL: "https://auth.example.com/", ClientID: "app",
	})
	// Bare URL — no query string.
	assert.Equal(t, "https://auth.example.com/auth/oidc/session/end",
		c.LogoutURL("", "", ""))

	// All params present.
	urlStr := c.LogoutURL("https://app.example.com", "eyJ.hint.sig", "s1")
	u, err := url.Parse(urlStr)
	require.NoError(t, err)
	assert.Equal(t, "/auth/oidc/session/end", u.Path)
	q := u.Query()
	assert.Equal(t, "https://app.example.com", q.Get("post_logout_redirect_uri"))
	assert.Equal(t, "eyJ.hint.sig", q.Get("id_token_hint"))
	assert.Equal(t, "s1", q.Get("state"))
}

func TestOAuthExchangeCodePropagatesHTTPError(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		http.Error(w, `{"error":"invalid_grant"}`, http.StatusBadRequest)
	}))
	defer srv.Close()
	c := auth.NewOAuthClient(auth.OAuthConfig{IssuerURL: srv.URL, ClientID: "app"})
	_, err := c.ExchangeCode(context.Background(), "code", "verifier")
	require.Error(t, err)
	assert.Contains(t, err.Error(), "400")
	assert.Contains(t, err.Error(), "invalid_grant")
}
