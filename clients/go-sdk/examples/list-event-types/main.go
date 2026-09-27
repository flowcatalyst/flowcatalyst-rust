// Command list-event-types is the smallest possible FlowCatalyst SDK
// example. It builds a *client.FlowCatalystClient against a platform
// URL, calls EventTypes().List, and prints the result.
//
// Two auth flavors are shown:
//
//   - Static bearer token (set via FC_TOKEN env). Fine for scripts.
//   - OAuth2 client_credentials (set FC_CLIENT_ID + FC_CLIENT_SECRET
//     + FC_ISSUER). The TokenProvider caches tokens until 60s before
//     expiry and refreshes on demand — wire this for long-running
//     services.
//
// # Run
//
//	FC_BASE_URL=https://api.flowcatalyst.io \
//	FC_TOKEN=eyJ...                          \
//	go run ./examples/list-event-types
//
// Or with client credentials:
//
//	FC_BASE_URL=https://api.flowcatalyst.io \
//	FC_ISSUER=https://auth.flowcatalyst.io  \
//	FC_CLIENT_ID=svc-app                    \
//	FC_CLIENT_SECRET=...                    \
//	go run ./examples/list-event-types
package main

import (
	"context"
	"errors"
	"fmt"
	"log"
	"os"

	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/auth"
	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/client"
)

func main() {
	baseURL := os.Getenv("FC_BASE_URL")
	if baseURL == "" {
		log.Fatal("FC_BASE_URL is required")
	}

	c := client.New(baseURL, pickAuthOption()...)

	// Optional filters — pass empty strings to omit. Three filter
	// args is the convention across List endpoints: scope-by-app,
	// scope-by-status, scope-by-client.
	app := os.Getenv("FC_APP")
	out, err := c.EventTypes().List(context.Background(), app, "", "")
	if err != nil {
		// APIError carries the HTTP status + body for non-2xx; the
		// errors.As pattern is the convention across the SDK for any
		// surface that wraps a remote error.
		var apiErr *client.APIError
		if errors.As(err, &apiErr) {
			log.Fatalf("platform returned %d: %s", apiErr.StatusCode, apiErr.Body)
		}
		log.Fatalf("list event types: %v", err)
	}

	for _, et := range out.Items {
		fmt.Printf("%s\t%s\t(application=%s)\n", et.Code, et.Name, et.Application)
	}
}

func pickAuthOption() []client.Option {
	if tok := os.Getenv("FC_TOKEN"); tok != "" {
		return []client.Option{client.WithToken(tok)}
	}
	issuer := os.Getenv("FC_ISSUER")
	clientID := os.Getenv("FC_CLIENT_ID")
	secret := os.Getenv("FC_CLIENT_SECRET")
	if issuer != "" && clientID != "" && secret != "" {
		cc := auth.NewClientCredentialsProvider(auth.ClientCredentialsConfig{
			IssuerURL:    issuer,
			ClientID:     clientID,
			ClientSecret: secret,
		})
		return []client.Option{client.WithTokenProvider(cc.Token)}
	}
	log.Fatal("set FC_TOKEN, or FC_ISSUER + FC_CLIENT_ID + FC_CLIENT_SECRET")
	return nil
}
