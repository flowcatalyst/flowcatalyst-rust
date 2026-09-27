package auth_test

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/auth"
)

func TestClientCredentialsProviderFetchesAndCaches(t *testing.T) {
	var calls atomic.Int32
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		require.NoError(t, r.ParseForm())
		assert.Equal(t, "client_credentials", r.PostForm.Get("grant_type"))
		assert.Equal(t, "svc-app", r.PostForm.Get("client_id"))
		assert.Equal(t, "shh", r.PostForm.Get("client_secret"))
		calls.Add(1)
		_ = json.NewEncoder(w).Encode(map[string]any{
			"access_token": "at_1",
			"token_type":   "Bearer",
			"expires_in":   3600,
		})
	}))
	defer srv.Close()

	p := auth.NewClientCredentialsProvider(auth.ClientCredentialsConfig{
		IssuerURL: srv.URL, ClientID: "svc-app", ClientSecret: "shh",
	})

	// First call → fetches.
	tok, err := p.Token(context.Background())
	require.NoError(t, err)
	assert.Equal(t, "at_1", tok)
	assert.Equal(t, int32(1), calls.Load())

	// Second call within the cached window → no new fetch.
	tok, err = p.Token(context.Background())
	require.NoError(t, err)
	assert.Equal(t, "at_1", tok)
	assert.Equal(t, int32(1), calls.Load())
}

func TestClientCredentialsProviderRefreshesAfterInvalidate(t *testing.T) {
	var calls atomic.Int32
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		n := calls.Add(1)
		_ = json.NewEncoder(w).Encode(map[string]any{
			"access_token": "at_" + string(rune('0'+n)),
			"token_type":   "Bearer",
			"expires_in":   3600,
		})
	}))
	defer srv.Close()

	p := auth.NewClientCredentialsProvider(auth.ClientCredentialsConfig{
		IssuerURL: srv.URL, ClientID: "app", ClientSecret: "s",
	})
	tok, err := p.Token(context.Background())
	require.NoError(t, err)
	assert.Equal(t, "at_1", tok)

	p.Invalidate()
	tok, err = p.Token(context.Background())
	require.NoError(t, err)
	assert.Equal(t, "at_2", tok)
}

func TestClientCredentialsProviderSurfacesHTTPError(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		http.Error(w, `{"error":"invalid_client"}`, http.StatusUnauthorized)
	}))
	defer srv.Close()

	p := auth.NewClientCredentialsProvider(auth.ClientCredentialsConfig{
		IssuerURL: srv.URL, ClientID: "app", ClientSecret: "s",
	})
	_, err := p.Token(context.Background())
	require.Error(t, err)
	assert.Contains(t, err.Error(), "401")
}

func TestClientCredentialsProviderRequestsScopesAndAudience(t *testing.T) {
	var captured struct {
		scope    string
		audience string
	}
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		require.NoError(t, r.ParseForm())
		captured.scope = r.PostForm.Get("scope")
		captured.audience = r.PostForm.Get("audience")
		_ = json.NewEncoder(w).Encode(map[string]any{
			"access_token": "at", "token_type": "Bearer", "expires_in": 60,
		})
	}))
	defer srv.Close()

	p := auth.NewClientCredentialsProvider(auth.ClientCredentialsConfig{
		IssuerURL: srv.URL, ClientID: "app", ClientSecret: "s",
		Scopes:   []string{"read", "write"},
		Audience: "api.example.com",
	})
	_, err := p.Token(context.Background())
	require.NoError(t, err)
	assert.Equal(t, "read write", captured.scope)
	assert.Equal(t, "api.example.com", captured.audience)
}
