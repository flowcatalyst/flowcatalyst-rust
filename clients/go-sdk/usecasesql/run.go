package usecasesql

import (
	"context"
	"database/sql"

	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/internal/sealed"
	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/usecase"
)

// TxScopedUnitOfWork is a UnitOfWork bound to an externally-orchestrated
// transaction opened by Run. Every commit performed against it joins
// that single transaction. The scoped UoW does NOT close the tx; the
// surrounding Run does (commits on Success, rolls back on Failure).
type TxScopedUnitOfWork struct {
	tx   *sql.Tx
	sink Sink
}

// WithTx invokes the callback with the underlying *sql.Tx so callers
// can run ad-hoc writes on the same tx as the outbox / audit rows.
func (s *TxScopedUnitOfWork) WithTx(ctx context.Context, fn func(*sql.Tx) error) error {
	return fn(s.tx)
}

// Sink returns the configured sink.
func (s *TxScopedUnitOfWork) Sink() Sink { return s.sink }

// Run opens one transaction on the DB, builds a TxScopedUnitOfWork
// bound to it, and invokes fn. Commits on Success, rolls back on
// Failure or panic.
func Run[R any](
	ctx context.Context,
	uow *UnitOfWork,
	fn func(*TxScopedUnitOfWork) usecase.Result[R],
) usecase.Result[R] {
	tx, err := uow.db.BeginTx(ctx, uow.opts)
	if err != nil {
		return usecase.Failure[R](usecase.Internal("TX_BEGIN", "could not open orchestration transaction", err))
	}
	defer func() {
		if r := recover(); r != nil {
			_ = tx.Rollback()
			panic(r)
		}
	}()

	scoped := &TxScopedUnitOfWork{tx: tx, sink: uow.sink}
	result := fn(scoped)

	if usecase.IsSuccess(result) {
		if err := tx.Commit(); err != nil {
			return usecase.Failure[R](usecase.Internal("TX_COMMIT", "could not commit orchestration tx", err))
		}
		return result
	}
	_ = tx.Rollback()
	return result
}

// CommitScoped is the Commit equivalent for a TxScopedUnitOfWork.
func CommitScoped[A usecase.HasID, E usecase.DomainEvent, C any](
	ctx context.Context,
	scoped *TxScopedUnitOfWork,
	aggregate *A,
	repo Persist[A],
	event E,
	command C,
) usecase.Result[E] {
	dbTx := newDbTx(scoped.tx)
	if err := repo.Persist(ctx, aggregate, dbTx); err != nil {
		return usecase.Failure[E](usecase.Internal("PERSIST", "repository persist failed", err))
	}
	if err := scoped.sink.WriteEvent(ctx, dbTx, event); err != nil {
		return usecase.Failure[E](usecase.Internal("EVENT_WRITE", "could not write domain event", err))
	}
	if err := scoped.sink.WriteAudit(ctx, dbTx, event, command); err != nil {
		return usecase.Failure[E](usecase.Internal("AUDIT_WRITE", "could not write audit log", err))
	}
	return usecase.Success[E](sealed.New(), event)
}

// CommitDeleteScoped is the CommitDelete equivalent for a TxScopedUnitOfWork.
func CommitDeleteScoped[A usecase.HasID, E usecase.DomainEvent, C any](
	ctx context.Context,
	scoped *TxScopedUnitOfWork,
	aggregate *A,
	repo Persist[A],
	event E,
	command C,
) usecase.Result[E] {
	dbTx := newDbTx(scoped.tx)
	if err := repo.Delete(ctx, aggregate, dbTx); err != nil {
		return usecase.Failure[E](usecase.Internal("DELETE", "repository delete failed", err))
	}
	if err := scoped.sink.WriteEvent(ctx, dbTx, event); err != nil {
		return usecase.Failure[E](usecase.Internal("EVENT_WRITE", "could not write domain event", err))
	}
	if err := scoped.sink.WriteAudit(ctx, dbTx, event, command); err != nil {
		return usecase.Failure[E](usecase.Internal("AUDIT_WRITE", "could not write audit log", err))
	}
	return usecase.Success[E](sealed.New(), event)
}

// EmitEventScoped is the EmitEvent equivalent for a TxScopedUnitOfWork.
func EmitEventScoped[E usecase.DomainEvent, C any](
	ctx context.Context,
	scoped *TxScopedUnitOfWork,
	event E,
	command C,
) usecase.Result[E] {
	dbTx := newDbTx(scoped.tx)
	if err := scoped.sink.WriteEvent(ctx, dbTx, event); err != nil {
		return usecase.Failure[E](usecase.Internal("EVENT_WRITE", "could not write domain event", err))
	}
	if err := scoped.sink.WriteAudit(ctx, dbTx, event, command); err != nil {
		return usecase.Failure[E](usecase.Internal("AUDIT_WRITE", "could not write audit log", err))
	}
	return usecase.Success[E](sealed.New(), event)
}
