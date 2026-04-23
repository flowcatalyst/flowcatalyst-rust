<?php

declare(strict_types=1);

namespace FlowCatalyst\UseCase;

/**
 * Commits a domain event (+ optional entity persistence and audit log)
 * atomically with respect to the outbox. The default implementation
 * `OutboxUnitOfWork` writes to the outbox table; the fc-outbox-processor
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
}
