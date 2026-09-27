// Command fc-sync is the declarative-reconciliation example. It builds
// a DefinitionSet describing the roles, event types, dispatch pools,
// subscriptions, and processes that one application "owns", then hands
// it to a Synchronizer that calls the per-category sync endpoints.
//
// One platform call per category. RemoveUnlisted=true asks the
// platform to archive SDK-sourced rows that aren't in this set;
// UI-sourced rows are never touched.
//
// # Run
//
//	FC_BASE_URL=https://api.flowcatalyst.io  \
//	FC_TOKEN=eyJ...                          \
//	go run ./examples/fc-sync
package main

import (
	"context"
	"fmt"
	"log"
	"os"

	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/client"
	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/sync"
)

const appCode = "orders"

func main() {
	c := client.New(
		mustEnv("FC_BASE_URL"),
		client.WithToken(mustEnv("FC_TOKEN")),
	)

	set := sync.ForApplication(appCode).
		// Roles — Name is the short form; the platform prefixes with
		// the app code to form the full role code (orders:admin etc).
		AddRole(sync.MakeRole("admin").
			WithDisplayName("Orders Admin").
			WithDescription("Full read/write access to orders.").
			WithPermissions("orders:read", "orders:write")).
		AddRole(sync.MakeRole("viewer").
			WithDisplayName("Orders Viewer").
			WithPermissions("orders:read").
			ClientManagedEnabled()).

		// Event types — Code is the full four-segment identifier
		// {app}:{domain}:{aggregate}:{event}.
		AddEventType(sync.MakeEventType(
			"orders:sales:order:placed", "Order placed").
			WithDescription("A new order has been accepted.")).
		AddEventType(sync.MakeEventType(
			"orders:sales:order:cancelled", "Order cancelled")).

		// Dispatch pools — concurrency control for downstream webhook
		// delivery.
		AddDispatchPool(sync.MakeDispatchPool("default", "Default pool").
			WithConcurrency(8).
			WithDescription("Catch-all for orders subscriptions.")).

		// Subscriptions — bind event types to a webhook target.
		AddSubscription(sync.MakeSubscription(
			"order-billing", "Forward placed orders to billing",
			"https://billing.example.com/webhooks/orders").
			WithDispatchPool("default").
			WithMode("Immediate").
			WithMaxRetries(5).
			Bind("orders:sales:order:placed", "")).

		// Processes — documentation-style entries (no runtime effect).
		AddProcess(sync.MakeProcess("place-order", "Place Order").
			WithDescription("End-to-end flow for placing an order."))

	r := sync.NewSynchronizer(c).Sync(
		context.Background(),
		set,
		sync.WithRemoveUnlisted(), // archive SDK-sourced rows not in the set
	)

	printSummary(r)
	if r.HasErrors() {
		os.Exit(1)
	}
}

func printSummary(r *sync.Result) {
	fmt.Printf("application=%s\n", r.ApplicationCode)
	for _, cat := range []struct {
		name string
		res  *sync.CategoryResult
	}{
		{"roles", r.Roles},
		{"event_types", r.EventTypes},
		{"subscriptions", r.Subscriptions},
		{"dispatch_pools", r.DispatchPools},
		{"principals", r.Principals},
		{"processes", r.Processes},
	} {
		if cat.res == nil {
			fmt.Printf("  %-15s skipped\n", cat.name)
			continue
		}
		if cat.res.IsError() {
			fmt.Printf("  %-15s ERROR: %s\n", cat.name, cat.res.Error)
			continue
		}
		fmt.Printf("  %-15s created=%d updated=%d deleted=%d\n",
			cat.name, cat.res.Created, cat.res.Updated, cat.res.Deleted)
	}
}

func mustEnv(k string) string {
	v := os.Getenv(k)
	if v == "" {
		log.Fatalf("%s is required", k)
	}
	return v
}
