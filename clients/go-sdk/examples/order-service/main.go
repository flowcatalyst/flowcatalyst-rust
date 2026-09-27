// Command order-service is a runnable end-to-end example of a
// FlowCatalyst SDK consumer app. It shows the assembly that the
// package docs describe one slice at a time:
//
//	HTTP request  →  command DTO
//	             →  ExecutionContext (principal + correlation)
//	             →  UseCase.Run  → Validate → Authorize → Execute
//	                                            ↓
//	                                       usecasepgx.Commit
//	                                       (single tx)
//	                                         ↳ OrderRepository.Persist
//	                                         ↳ outboxpgx.Sink.WriteEvent
//	                                         ↳ outboxpgx.Sink.WriteAudit
//	                                       commit / rollback
//	             →  usecase.Into(result) → 200 / 400 / 409 / 500
//
// The Sink writes to outbox_messages, which the platform's
// fc-outbox-processor forwards to /api/events/batch — so the
// OrderPlaced event lands in the platform's msg_events table without
// any direct platform calls from this service.
//
// # Schema
//
// Bring up Postgres and apply:
//
//	-- Outbox table (also created by fc-outbox-processor's migrations).
//	CREATE TABLE IF NOT EXISTS outbox_messages (
//	    id              TEXT PRIMARY KEY,
//	    type            TEXT NOT NULL,
//	    message_group   TEXT,
//	    payload         TEXT NOT NULL,
//	    status          INT  NOT NULL DEFAULT 0,
//	    retry_count     INT  NOT NULL DEFAULT 0,
//	    client_id       TEXT,
//	    payload_size    INT  NOT NULL,
//	    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
//	    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
//	);
//
//	-- Demo aggregate table.
//	CREATE TABLE IF NOT EXISTS orders (
//	    id          TEXT PRIMARY KEY,
//	    customer_id TEXT NOT NULL,
//	    total_cents BIGINT NOT NULL,
//	    placed_at   TIMESTAMPTZ NOT NULL
//	);
//
// # Run
//
//	FC_DATABASE_URL=postgres://localhost:5432/orders go run ./examples/order-service
//	curl -XPOST http://localhost:8080/orders \
//	    -H 'X-Principal-ID: prn_demo' \
//	    -H 'Content-Type: application/json' \
//	    -d '{"customerId":"cus_42","totalCents":1500}'
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"os"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/outboxpgx"
	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/tsid"
	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/usecase"
	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/usecasepgx"
)

// ───────────────────────────────────────────────────────────────────
// Domain
// ───────────────────────────────────────────────────────────────────

// Order is the aggregate. It satisfies usecase.HasID via the IDStr
// method so UnitOfWork can identify rows without reflection.
type Order struct {
	ID         string
	CustomerID string
	TotalCents int64
	PlacedAt   time.Time
}

func (o Order) IDStr() string { return o.ID }

// OrderRepository owns the SQL for the orders table. Implements
// usecasepgx.Persist[Order] — the "aggregates don't persist
// themselves" rule applies in Go too.
type OrderRepository struct{}

func (OrderRepository) Persist(ctx context.Context, o *Order, tx *usecasepgx.DbTx) error {
	_, err := tx.Inner().Exec(ctx,
		`INSERT INTO orders (id, customer_id, total_cents, placed_at)
		 VALUES ($1, $2, $3, $4)
		 ON CONFLICT (id) DO UPDATE
		    SET customer_id = EXCLUDED.customer_id,
		        total_cents = EXCLUDED.total_cents`,
		o.ID, o.CustomerID, o.TotalCents, o.PlacedAt)
	return err
}

func (OrderRepository) Delete(ctx context.Context, o *Order, tx *usecasepgx.DbTx) error {
	_, err := tx.Inner().Exec(ctx, `DELETE FROM orders WHERE id = $1`, o.ID)
	return err
}

// OrderPlaced is the domain event emitted on a successful PlaceOrder.
// Embeds EventMetadata so each accessor method delegates to the
// embedded fields — that's the pattern the comment in
// usecase/domain_event.go describes.
type OrderPlaced struct {
	usecase.EventMetadata
	CustomerID string `json:"customerId"`
	TotalCents int64  `json:"totalCents"`
}

// Compile-time check.
var _ usecase.DomainEvent = OrderPlaced{}

func (e OrderPlaced) EventID() string       { return e.EventMetadata.EventID }
func (e OrderPlaced) EventType() string     { return e.EventMetadata.Type }
func (e OrderPlaced) SpecVersion() string   { return e.EventMetadata.SpecVersion }
func (e OrderPlaced) Source() string        { return e.EventMetadata.Source }
func (e OrderPlaced) Subject() string       { return e.EventMetadata.Subject }
func (e OrderPlaced) Time() time.Time       { return e.EventMetadata.OccurredAt }
func (e OrderPlaced) PrincipalID() string   { return e.EventMetadata.PrincipalID }
func (e OrderPlaced) CorrelationID() string { return e.EventMetadata.CorrelationID }
func (e OrderPlaced) CausationID() string   { return e.EventMetadata.CausationID }
func (e OrderPlaced) ExecutionID() string   { return e.EventMetadata.ExecutionID }
func (e OrderPlaced) MessageGroup() string  { return e.EventMetadata.MessageGroup }

func (e OrderPlaced) ToDataJSON() ([]byte, error) {
	return json.Marshal(struct {
		CustomerID string `json:"customerId"`
		TotalCents int64  `json:"totalCents"`
	}{e.CustomerID, e.TotalCents})
}

// ───────────────────────────────────────────────────────────────────
// Use case
// ───────────────────────────────────────────────────────────────────

// PlaceOrderCommand is the input DTO. Use-case-shaped — not necessarily
// 1:1 with the HTTP request body.
type PlaceOrderCommand struct {
	CustomerID string
	TotalCents int64
}

// PlaceOrder is the use case. The pipeline Validate → Authorize →
// Execute is called by usecase.Run; handlers never invoke them
// individually.
type PlaceOrder struct {
	UoW  *usecasepgx.UnitOfWork
	Repo OrderRepository
}

func (PlaceOrder) Validate(_ context.Context, cmd PlaceOrderCommand) error {
	if cmd.CustomerID == "" {
		return usecase.Validation("CUSTOMER_REQUIRED", "customerId is required")
	}
	if cmd.TotalCents <= 0 {
		return usecase.Validation("TOTAL_INVALID", "totalCents must be positive")
	}
	return nil
}

func (PlaceOrder) Authorize(_ context.Context, _ PlaceOrderCommand, ec usecase.ExecutionContext) error {
	// Resource-level authorization happens here. Anchor checks /
	// permission checks belong in the HTTP layer; this is the
	// per-resource gate (e.g. "may this principal place orders for
	// this client?"). For a demo we just require a principal.
	if ec.PrincipalID == "" {
		return usecase.Authorization("PRINCIPAL_REQUIRED", "no acting principal")
	}
	return nil
}

func (u PlaceOrder) Execute(ctx context.Context, cmd PlaceOrderCommand, ec usecase.ExecutionContext) usecase.Result[OrderPlaced] {
	order := &Order{
		ID:         tsid.GenerateWithPrefix("ord"),
		CustomerID: cmd.CustomerID,
		TotalCents: cmd.TotalCents,
		PlacedAt:   time.Now().UTC(),
	}
	event := OrderPlaced{
		EventMetadata: newEventMetadata(ec,
			"orders:sales:order:placed",          // event type
			"orders:sales",                       // source
			"orders.order."+order.ID,             // subject
		),
		CustomerID: order.CustomerID,
		TotalCents: order.TotalCents,
	}

	// One transaction:
	//   1. OrderRepository.Persist writes the row
	//   2. outboxpgx.Sink.WriteEvent writes the outbox EVENT row
	//   3. outboxpgx.Sink.WriteAudit writes the outbox AUDIT_LOG row
	//      (no-op here — AuditEnabled is false by default for consumer apps)
	//   4. Tx commits → fc-outbox-processor picks up the event next poll
	return usecasepgx.Commit(ctx, u.UoW, order, u.Repo, event, cmd)
}

// newEventMetadata is the per-app helper for stamping CloudEvents
// fields. SDKs that want to use TSIDs instead of UUIDs for eventId
// can swap uuid.NewString for tsid.Generate(tsid.Event).
func newEventMetadata(ec usecase.ExecutionContext, eventType, source, subject string) usecase.EventMetadata {
	return usecase.EventMetadata{
		EventID:       tsid.Generate(tsid.Event),
		SpecVersion:   "1.0",
		Source:        source,
		Type:          eventType,
		Subject:       subject,
		OccurredAt:    time.Now().UTC(),
		CorrelationID: ec.CorrelationID,
		CausationID:   ec.CausationID,
		PrincipalID:   ec.PrincipalID,
		ExecutionID:   ec.ExecutionID,
		MessageGroup:  usecase.BuildMessageGroup("orders", "order", usecase.ExtractEntityID(subject)),
	}
}

// ───────────────────────────────────────────────────────────────────
// HTTP layer
// ───────────────────────────────────────────────────────────────────

type placeOrderHandler struct {
	uc PlaceOrder
}

type placeOrderBody struct {
	CustomerID string `json:"customerId"`
	TotalCents int64  `json:"totalCents"`
}

func (h placeOrderHandler) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	var body placeOrderBody
	if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
		http.Error(w, `{"error":"invalid body"}`, http.StatusBadRequest)
		return
	}
	cmd := PlaceOrderCommand{CustomerID: body.CustomerID, TotalCents: body.TotalCents}

	// In a real app the principal comes from a validated bearer
	// token (auth.TokenValidator.ValidateBearer); we read a header
	// for clarity.
	ec := usecase.NewExecutionContext(r.Header.Get("X-Principal-ID"))
	if cid := r.Header.Get("X-Correlation-ID"); cid != "" {
		ec = usecase.WithCorrelation(ec.PrincipalID, cid)
	}

	event, err := usecase.Into(usecase.Run[PlaceOrderCommand, OrderPlaced](r.Context(), h.uc, cmd, ec))
	if err != nil {
		writeError(w, err)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusCreated)
	_ = json.NewEncoder(w).Encode(map[string]string{
		"eventId":     event.EventID(),
		"subject":     event.Subject(),
		"correlation": event.CorrelationID(),
	})
}

// writeError maps usecase.Error → HTTP status + JSON body.
func writeError(w http.ResponseWriter, err error) {
	w.Header().Set("Content-Type", "application/json")
	if uErr := usecase.AsError(err); uErr != nil {
		w.WriteHeader(uErr.HTTPStatus())
		_ = json.NewEncoder(w).Encode(map[string]string{
			"code":    uErr.Code,
			"message": uErr.Message,
		})
		return
	}
	w.WriteHeader(http.StatusInternalServerError)
	_, _ = w.Write([]byte(`{"error":"internal"}`))
}

// ───────────────────────────────────────────────────────────────────
// main: wire everything
// ───────────────────────────────────────────────────────────────────

func main() {
	ctx := context.Background()
	dsn := os.Getenv("FC_DATABASE_URL")
	if dsn == "" {
		dsn = "postgres://localhost:5432/orders?sslmode=disable"
	}
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		log.Fatalf("connect: %v", err)
	}
	defer pool.Close()
	if err := pool.Ping(ctx); err != nil {
		log.Fatalf("ping: %v", err)
	}

	// outboxpgx.Sink writes events to outbox_messages.
	// fc-outbox-processor forwards them to the platform's
	// /api/events/batch — so this service never calls the platform.
	sink := outboxpgx.NewSink(outboxpgx.DefaultConfig())
	uow := usecasepgx.New(pool, sink)
	uc := PlaceOrder{UoW: uow, Repo: OrderRepository{}}

	mux := http.NewServeMux()
	mux.Handle("POST /orders", placeOrderHandler{uc: uc})
	addr := ":8080"
	fmt.Println("listening on", addr)
	if err := http.ListenAndServe(addr, mux); err != nil {
		log.Fatal(err)
	}
}
