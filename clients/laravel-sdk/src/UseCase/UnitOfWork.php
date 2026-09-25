<?php

declare(strict_types=1);

namespace FlowCatalyst\UseCase;

/**
 * Commits a domain event (+ optional entity persistence and audit log)
 * atomically with respect to the outbox. The default implementation
 * `OutboxUnitOfWork` writes to the outbox table; the outbox poller
 * then forwards it to the FlowCatalyst platform.
 */
interface UnitOfWork
{
    /**
     * Commit a domain event. The optional `$persist` callable runs before
     * the event is enqueued — wrap the whole call in your own DB transaction
     * using a tx-aware `OutboxDriver` for strict atomicity.
     *
     * @template T of DomainEvent
     * @param T $event
     * @param callable():void|null $persist
     * @return Result<T>
     */
    public function commit(
        DomainEvent $event,
        mixed $command,
        ?callable $persist = null,
    ): Result;

    /**
     * Convenience mirror of the platform signature — same behaviour as
     * `commit`, with the aggregate parameter kept for symmetry.
     *
     * @template T of DomainEvent
     * @param T $event
     * @param callable():void|null $persist
     * @return Result<T>
     */
    public function commitAggregate(
        Aggregate $aggregate,
        DomainEvent $event,
        mixed $command,
        ?callable $persist = null,
    ): Result;

    /**
     * @template T of DomainEvent
     * @param T $event
     * @param callable():void|null $persist
     * @return Result<T>
     */
    public function commitDelete(
        Aggregate $aggregate,
        DomainEvent $event,
        mixed $command,
        ?callable $persist = null,
    ): Result;

    /**
     * Emit an event with no entity change (e.g. UserLoggedIn).
     *
     * @template T of DomainEvent
     * @param T $event
     * @return Result<T>
     */
    public function emitEvent(DomainEvent $event, mixed $command): Result;

    /**
     * Run the callback inside a single owned transaction on the unit of work's
     * connection: commit when the callback returns normally, roll back (and
     * rethrow) when it throws — the same `DB::transaction(...)` contract, so the
     * outbox write and any aggregate writes on the connection commit or roll
     * back together. {@link Runner} uses this to apply a {@link Plan} atomically.
     *
     * @template R
     * @param callable():R $callback
     * @return R
     */
    public function transaction(callable $callback): mixed;
}
