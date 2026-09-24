package client_test

import (
	"context"
	"net/http"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/flowcatalyst/flowcatalyst/clients/go-sdk/client"
)

// Source is a plain string, not a closed enum — a FUNCTION-sourced
// subscription (function promotion) must decode the same as any other
// source value.
func TestSubscriptionGetDecodesFunctionSource(t *testing.T) {
	srv, seen := newMockSrv(t, `{
		"id": "sub_1",
		"code": "orders-shipped",
		"name": "Orders Shipped",
		"endpoint": "https://example.com/webhook",
		"source": "FUNCTION",
		"status": "ACTIVE",
		"mode": "IMMEDIATE",
		"createdAt": "2026-01-01T00:00:00Z",
		"updatedAt": "2026-01-01T00:00:00Z"
	}`)
	c := client.New(srv.URL)

	sub, err := c.Subscriptions().Get(context.Background(), "sub_1")
	require.NoError(t, err)
	assert.Equal(t, http.MethodGet, seen.method)
	assert.Equal(t, "FUNCTION", sub.Source)
}
