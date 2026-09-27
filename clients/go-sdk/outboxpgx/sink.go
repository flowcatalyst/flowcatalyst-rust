package outboxpgx

import (
	"context"
	"encoding/json"
	"reflect"

	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/tsid"
	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/usecase"
	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/usecasepgx"
)

// Config configures the outbox Sink.
type Config struct {
	// TableName is the outbox table. Default: "outbox_messages".
	TableName string
	// ClientID scopes outbox rows to a tenant. May be empty.
	ClientID string
	// AuditEnabled writes audit log rows alongside every event. The
	// platform always audits its control plane writes; consumer apps
	// should only enable this for admin / human-initiated operations,
	// not for every transactional event.
	AuditEnabled bool
}

// DefaultConfig returns a Config with sensible defaults.
func DefaultConfig() Config {
	return Config{
		TableName:    "outbox_messages",
		AuditEnabled: false,
	}
}

// Sink writes domain events and audit logs to the outbox_messages
// table. Satisfies usecasepgx.Sink.
type Sink struct {
	cfg Config
}

// NewSink builds a Sink. If cfg.TableName is empty, defaults to
// "outbox_messages".
func NewSink(cfg Config) *Sink {
	if cfg.TableName == "" {
		cfg.TableName = "outbox_messages"
	}
	return &Sink{cfg: cfg}
}

// Compile-time check that *Sink satisfies usecasepgx.Sink.
var _ usecasepgx.Sink = (*Sink)(nil)

// WriteEvent inserts an EVENT row into outbox_messages.
func (s *Sink) WriteEvent(ctx context.Context, tx *usecasepgx.DbTx, event usecase.DomainEvent) error {
	payload, err := buildEventPayload(event)
	if err != nil {
		return err
	}
	payloadStr := string(payload)

	id := newOutboxID()
	mg := event.MessageGroup()

	query := "INSERT INTO " + s.cfg.TableName + ` (id, type, message_group, payload, status, retry_count, created_at, updated_at, client_id, payload_size)
VALUES ($1, 'EVENT', $2, $3, 0, 0, NOW(), NOW(), $4, $5)`

	_, err = tx.Inner().Exec(ctx, query, id, nullableString(mg), payloadStr, nullableString(s.cfg.ClientID), len(payloadStr))
	return err
}

// WriteAudit inserts an AUDIT_LOG row into outbox_messages, if audit
// logging is enabled. No-op otherwise.
func (s *Sink) WriteAudit(ctx context.Context, tx *usecasepgx.DbTx, event usecase.DomainEvent, command any) error {
	if !s.cfg.AuditEnabled {
		return nil
	}
	payload, err := buildAuditPayload(event, command)
	if err != nil {
		return err
	}
	payloadStr := string(payload)

	id := newOutboxID()
	mg := event.MessageGroup()

	query := "INSERT INTO " + s.cfg.TableName + ` (id, type, message_group, payload, status, retry_count, created_at, updated_at, client_id, payload_size)
VALUES ($1, 'AUDIT_LOG', $2, $3, 0, 0, NOW(), NOW(), $4, $5)`

	_, err = tx.Inner().Exec(ctx, query, id, nullableString(mg), payloadStr, nullableString(s.cfg.ClientID), len(payloadStr))
	return err
}

// buildEventPayload serializes a domain event into the snake_case JSON
// shape the fc-outbox-processor parses. Matches the Rust SDK byte-for-byte.
func buildEventPayload(event usecase.DomainEvent) ([]byte, error) {
	data, err := event.ToDataJSON()
	if err != nil {
		return nil, err
	}
	var dataAny any
	if len(data) == 0 {
		dataAny = map[string]any{}
	} else if err := json.Unmarshal(data, &dataAny); err != nil {
		// If the event's ToDataJSON returns non-object JSON, store it as-is.
		dataAny = map[string]any{}
	}

	payload := map[string]any{
		"event_type":        event.EventType(),
		"spec_version":      event.SpecVersion(),
		"source":            event.Source(),
		"subject":           event.Subject(),
		"data":              dataAny,
		"correlation_id":    event.CorrelationID(),
		"causation_id":      event.CausationID(),
		"deduplication_id":  event.EventType() + "-" + event.EventID(),
		"message_group":     event.MessageGroup(),
		"context_data": []map[string]string{
			{"key": "principalId", "value": event.PrincipalID()},
			{"key": "aggregateType", "value": usecase.ExtractAggregateType(event.Subject())},
		},
	}
	return json.Marshal(payload)
}

// buildAuditPayload serializes an audit log row.
func buildAuditPayload(event usecase.DomainEvent, command any) ([]byte, error) {
	cmdJSON, err := json.Marshal(command)
	if err != nil {
		return nil, err
	}

	cmdName := "Unknown"
	if command != nil {
		t := reflect.TypeOf(command)
		if t.Kind() == reflect.Ptr {
			t = t.Elem()
		}
		if t.Name() != "" {
			cmdName = t.Name()
		}
	}

	payload := map[string]any{
		"entity_type":    usecase.ExtractAggregateType(event.Subject()),
		"entity_id":      usecase.ExtractEntityID(event.Subject()),
		"operation":      cmdName,
		"operation_json": json.RawMessage(cmdJSON),
		"principal_id":   event.PrincipalID(),
		"performed_at":   event.Time().UTC().Format("2006-01-02T15:04:05.000000000Z07:00"),
	}
	return json.Marshal(payload)
}

// newOutboxID returns a 13-char Crockford Base32 TSID. Matches the Rust
// SDK's TsidGenerator::generate_untyped() so all four SDKs produce
// identical wire IDs for outbox rows.
func newOutboxID() string { return tsid.GenerateUntyped() }

func nullableString(s string) any {
	if s == "" {
		return nil
	}
	return s
}
