package usecasesql

import (
	"context"

	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/usecase"
)

// Sink is the destination for domain events and audit logs. The
// UnitOfWork calls into this interface inside the transaction it has
// open. The standard consumer implementation is outboxsql.Sink, which
// writes to outbox_messages.
type Sink interface {
	// WriteEvent appends the domain event to its destination, using
	// the already-open transaction. Must not commit or roll back.
	WriteEvent(ctx context.Context, tx *DbTx, event usecase.DomainEvent) error

	// WriteAudit appends an audit log row for (event, command), using
	// the same open transaction. Implementations may no-op when audit
	// logging is disabled.
	WriteAudit(ctx context.Context, tx *DbTx, event usecase.DomainEvent, command any) error
}
