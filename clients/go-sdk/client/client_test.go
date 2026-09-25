package client_test

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/flowcatalyst/flowcatalyst/clients/go-sdk/client"
)

func TestClientGetAttachesBearerToken(t *testing.T) {
	var seen string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		seen = r.Header.Get("Authorization")
		_ = json.NewEncoder(w).Encode(map[string]string{"hello": "world"})
	}))
	defer srv.Close()

	c := client.New(srv.URL, client.WithToken("abc123"))
	var out map[string]string
	require.NoError(t, c.Get(context.Background(), "/api/x", &out))
	assert.Equal(t, "Bearer abc123", seen)
	assert.Equal(t, "world", out["hello"])
}

func TestClientReturnsAPIErrorOnNon2xx(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusNotFound)
		_, _ = w.Write([]byte(`{"error":"EVENT_TYPE_NOT_FOUND","message":"missing"}`))
	}))
	defer srv.Close()

	c := client.New(srv.URL)
	err := c.Get(context.Background(), "/api/event-types/missing", nil)
	require.Error(t, err)

	var apiErr *client.APIError
	require.True(t, errors.As(err, &apiErr))
	assert.Equal(t, http.StatusNotFound, apiErr.StatusCode)
	assert.Equal(t, "EVENT_TYPE_NOT_FOUND", apiErr.Code())
	assert.Equal(t, "missing", apiErr.Message())
	assert.True(t, apiErr.IsNotFound())
	assert.False(t, apiErr.Retryable())
}

func TestAPIErrorCodeReadsTheErrorFieldThenCode(t *testing.T) {
	for _, tc := range []struct {
		body, code, message string
	}{
		// The platform's envelope: the code is in `error`.
		{`{"error":"EVENT_TYPE_NOT_FOUND","message":"missing","details":{"id":"x"}}`, "EVENT_TYPE_NOT_FOUND", "missing"},
		// Only `code`: still read.
		{`{"code":"BUSY","message":"later"}`, "BUSY", "later"},
		// Both: `error` wins.
		{`{"error":"CODE_EXISTS","code":"OTHER"}`, "CODE_EXISTS", ""},
		{`not json`, "", ""},
		{``, "", ""},
	} {
		e := &client.APIError{StatusCode: http.StatusConflict, Body: tc.body}
		assert.Equal(t, tc.code, e.Code(), tc.body)
		assert.Equal(t, tc.message, e.Message(), tc.body)
	}
}

func TestClientRetriesOn503(t *testing.T) {
	var calls atomic.Int32
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		n := calls.Add(1)
		if n < 3 {
			w.WriteHeader(http.StatusServiceUnavailable)
			_, _ = w.Write([]byte(`{"code":"BUSY"}`))
			return
		}
		_ = json.NewEncoder(w).Encode(map[string]string{"ok": "yes"})
	}))
	defer srv.Close()

	c := client.New(srv.URL, client.WithRetry(5, 1*time.Millisecond))
	var out map[string]string
	require.NoError(t, c.Get(context.Background(), "/x", &out))
	assert.Equal(t, "yes", out["ok"])
	assert.Equal(t, int32(3), calls.Load())
}

func TestEncodeQueryOmitsEmpty(t *testing.T) {
	assert.Equal(t, "", client.EncodeQuery())
	assert.Equal(t, "", client.EncodeQuery("a", "", "b", ""))
	// Single value: deterministic ordering.
	assert.Equal(t, "?a=1", client.EncodeQuery("a", "1"))
	// url.Values sorts keys, so output ordering is stable.
	q := client.EncodeQuery("status", "active", "clientId", "clt_1")
	assert.Equal(t, "?clientId=clt_1&status=active", q)
}

func TestAPIErrorRetryableForStatuses(t *testing.T) {
	for _, s := range []int{408, 425, 429, 500, 502, 503, 504} {
		e := &client.APIError{StatusCode: s}
		assert.True(t, e.Retryable(), "status %d should be retryable", s)
	}
	for _, s := range []int{400, 401, 403, 404, 409, 422} {
		e := &client.APIError{StatusCode: s}
		assert.False(t, e.Retryable(), "status %d should NOT be retryable", s)
	}
}
