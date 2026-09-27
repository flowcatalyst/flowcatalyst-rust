package usecasepgx

import (
	"context"

	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/internal/sealed"
	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/usecase"
	"github.com/jackc/pgx/v5"
)

// TxScopedUnitOfWork is a UnitOfWork bound to an externally-orchestrated
// transaction opened by Run. Every commit performed against it joins
// that single transaction. The scoped UoW does NOT close the tx; the
// surrounding Run does (commits on Success, rolls back on Failure).
//
// Use TxScopedUnitOfWork.WithTx for ad-hoc writes that must be atomic
// with the events the scoped UoW produces (e.g. updating a non-aggregate
// row, raw SQL on a join table).
type TxScopedUnitOfWork struct {
	tx   pgx.Tx
	sink Sink
}

// WithTx invokes the callback with the underlying pgx.Tx so callers can
// run ad-hoc writes (raw SQL, repository methods) on the same tx as the
// outbox / audit rows. Throws propagate to trigger rollback of the
// entire Run block.
func (s *TxScopedUnitOfWork) WithTx(ctx context.Context, fn func(pgx.Tx) error) error {
	return fn(s.tx)
}

// Sink returns the configured sink. Useful when constructing other use
// cases that take a Sink directly.
func (s *TxScopedUnitOfWork) Sink() Sink { return s.sink }

// Run opens one transaction on the pool, builds a TxScopedUnitOfWork
// bound to it, and invokes fn. Commits on Success, rolls back on
// Failure or panic. Use this when a handler needs to compose multiple
// aggregate writes (or non-aggregate writes via WithTx) atomically.
//
// Run is a free function because Go does not allow type parameters on
// methods. Pass the UnitOfWork as the first argument:
//
//	result := usecasepgx.Run(ctx, uow, func(s *usecasepgx.TxScopedUnitOfWork) usecase.Result[OrderShipped] {
//	    if r := usecasepgx.CommitScoped(ctx, s, &order, orderRepo, shipEvent, shipCmd); !usecase.IsSuccess(r) {
//	        return r
//	    }
//	    return usecasepgx.CommitScoped(ctx, s, &ledger, ledgerRepo, debitEvent, debitCmd)
//	})
func Run[R any](
	ctx context.Context,
	uow *UnitOfWork,
	fn func(*TxScopedUnitOfWork) usecase.Result[R],
) usecase.Result[R] {
	tx, err := uow.pool.Begin(ctx)
	if err != nil {
		return usecase.Failure[R](usecase.Internal("TX_BEGIN", "could not open orchestration transaction", err))
	}
	defer func() {
		if r := recover(); r != nil {
			_ = tx.Rollback(ctx)
			panic(r)
		}
	}()

	scoped := &TxScopedUnitOfWork{tx: tx, sink: uow.sink}
	result := fn(scoped)

	if usecase.IsSuccess(result) {
		if err := tx.Commit(ctx); err != nil {
			return usecase.Failure[R](usecase.Internal("TX_COMMIT", "could not commit orchestration tx", err))
		}
		return result
	}
	_ = tx.Rollback(ctx)
	return result
}

// CommitScoped is the Commit equivalent for a TxScopedUnitOfWork. It
// appends the aggregate write + event + audit to the open transaction
// but does NOT commit; the surrounding Run does.
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
