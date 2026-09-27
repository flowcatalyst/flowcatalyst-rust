package usecasepgx

import (
	"context"
	"fmt"

	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/internal/sealed"
	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/usecase"
)

// Commit upserts the aggregate via its repository, writes the domain
// event and audit log via the configured Sink — all in one transaction.
//
// This is one of the few public paths to a Success-valued
// usecase.Result outside the SDK. The seal on usecase.Success is
// satisfied here by importing internal/sealed.
func Commit[A usecase.HasID, E usecase.DomainEvent, C any](
	ctx context.Context,
	uow *UnitOfWork,
	aggregate *A,
	repo Persist[A],
	event E,
	command C,
) usecase.Result[E] {
	tx, err := uow.pool.Begin(ctx)
	if err != nil {
		return usecase.Failure[E](usecase.Internal("TX_BEGIN", "could not open transaction", err))
	}
	defer func() { _ = tx.Rollback(ctx) }()

	dbTx := newDbTx(tx)
	if err := repo.Persist(ctx, aggregate, dbTx); err != nil {
		return usecase.Failure[E](usecase.Internal("PERSIST", "repository persist failed", err))
	}
	if err := uow.sink.WriteEvent(ctx, dbTx, event); err != nil {
		return usecase.Failure[E](usecase.Internal("EVENT_WRITE", "could not write domain event", err))
	}
	if err := uow.sink.WriteAudit(ctx, dbTx, event, command); err != nil {
		return usecase.Failure[E](usecase.Internal("AUDIT_WRITE", "could not write audit log", err))
	}
	if err := tx.Commit(ctx); err != nil {
		return usecase.Failure[E](usecase.Internal("TX_COMMIT", "could not commit transaction", err))
	}

	return usecase.Success[E](sealed.New(), event)
}

// CommitDelete deletes the aggregate via its repository and emits the
// deletion event + audit log atomically.
func CommitDelete[A usecase.HasID, E usecase.DomainEvent, C any](
	ctx context.Context,
	uow *UnitOfWork,
	aggregate *A,
	repo Persist[A],
	event E,
	command C,
) usecase.Result[E] {
	tx, err := uow.pool.Begin(ctx)
	if err != nil {
		return usecase.Failure[E](usecase.Internal("TX_BEGIN", "could not open transaction", err))
	}
	defer func() { _ = tx.Rollback(ctx) }()

	dbTx := newDbTx(tx)
	if err := repo.Delete(ctx, aggregate, dbTx); err != nil {
		return usecase.Failure[E](usecase.Internal("DELETE", "repository delete failed", err))
	}
	if err := uow.sink.WriteEvent(ctx, dbTx, event); err != nil {
		return usecase.Failure[E](usecase.Internal("EVENT_WRITE", "could not write domain event", err))
	}
	if err := uow.sink.WriteAudit(ctx, dbTx, event, command); err != nil {
		return usecase.Failure[E](usecase.Internal("AUDIT_WRITE", "could not write audit log", err))
	}
	if err := tx.Commit(ctx); err != nil {
		return usecase.Failure[E](usecase.Internal("TX_COMMIT", "could not commit transaction", err))
	}

	return usecase.Success[E](sealed.New(), event)
}

// EmitEvent writes a domain event + audit log without an entity change.
// Used for events that don't modify an entity directly (e.g.
// UserLoggedIn).
func EmitEvent[E usecase.DomainEvent, C any](
	ctx context.Context,
	uow *UnitOfWork,
	event E,
	command C,
) usecase.Result[E] {
	tx, err := uow.pool.Begin(ctx)
	if err != nil {
		return usecase.Failure[E](usecase.Internal("TX_BEGIN", "could not open transaction", err))
	}
	defer func() { _ = tx.Rollback(ctx) }()

	dbTx := newDbTx(tx)
	if err := uow.sink.WriteEvent(ctx, dbTx, event); err != nil {
		return usecase.Failure[E](usecase.Internal("EVENT_WRITE", "could not write domain event", err))
	}
	if err := uow.sink.WriteAudit(ctx, dbTx, event, command); err != nil {
		return usecase.Failure[E](usecase.Internal("AUDIT_WRITE", "could not write audit log", err))
	}
	if err := tx.Commit(ctx); err != nil {
		return usecase.Failure[E](usecase.Internal("TX_COMMIT", "could not commit transaction", err))
	}

	return usecase.Success[E](sealed.New(), event)
}

// CommitAll upserts a batch of aggregates of the same type via one
// repository and emits a single summary event + audit log. Use when one
// logical operation touches many rows (e.g. toggling client → application
// enablement).
func CommitAll[A usecase.HasID, E usecase.DomainEvent, C any](
	ctx context.Context,
	uow *UnitOfWork,
	aggregates []A,
	repo Persist[A],
	event E,
	command C,
) usecase.Result[E] {
	tx, err := uow.pool.Begin(ctx)
	if err != nil {
		return usecase.Failure[E](usecase.Internal("TX_BEGIN", "could not open transaction", err))
	}
	defer func() { _ = tx.Rollback(ctx) }()

	dbTx := newDbTx(tx)
	for i := range aggregates {
		if err := repo.Persist(ctx, &aggregates[i], dbTx); err != nil {
			return usecase.Failure[E](usecase.Internal("PERSIST_BATCH", fmt.Sprintf("persist failed at index %d", i), err))
		}
	}
	if err := uow.sink.WriteEvent(ctx, dbTx, event); err != nil {
		return usecase.Failure[E](usecase.Internal("EVENT_WRITE", "could not write domain event", err))
	}
	if err := uow.sink.WriteAudit(ctx, dbTx, event, command); err != nil {
		return usecase.Failure[E](usecase.Internal("AUDIT_WRITE", "could not write audit log", err))
	}
	if err := tx.Commit(ctx); err != nil {
		return usecase.Failure[E](usecase.Internal("TX_COMMIT", "could not commit transaction", err))
	}

	return usecase.Success[E](sealed.New(), event)
}
