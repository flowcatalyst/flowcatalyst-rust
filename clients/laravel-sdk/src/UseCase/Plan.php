<?php

declare(strict_types=1);

namespace FlowCatalyst\UseCase;

/**
 * Plan — a pending change + the domain event it produces, built by an
 * {@link Operation}'s `execute` phase but NOT yet committed. {@link Runner} is
 * the only thing that applies a Plan, and it does so inside one transaction
 * (aggregate write + event + optional audit, atomically).
 *
 * Plan is sealed: its constructor is private, so the only ways to build one are
 * {@link Plan::save()} / {@link Plan::delete()} / {@link Plan::emit()}. An
 * operation therefore cannot reach the database except by returning a Plan and
 * letting the Runner apply it. (PHP has no module-private visibility, so the
 * seal is convention-level — but the structure carries the invariant: `execute`
 * returns a Plan and only the Runner applies it.) The PHP analogue of the Go
 * SDK's sealed `Plan[E]`.
 *
 * @template TEvent of DomainEvent
 */
final class Plan
{
    private const KIND_SAVE = 'save';
    private const KIND_DELETE = 'delete';
    private const KIND_EMIT = 'emit';

    private function __construct(
        private readonly string $kind,
        private readonly DomainEvent $event,
        private readonly ?Aggregate $aggregate = null,
        private readonly ?Repo $repo = null,
    ) {}

    /**
     * Plan an aggregate upsert with its domain event (create / update).
     *
     * @template T of DomainEvent
     * @param T $event
     * @return self<T>
     */
    public static function save(Aggregate $aggregate, Repo $repo, DomainEvent $event): self
    {
        return new self(self::KIND_SAVE, $event, $aggregate, $repo);
    }

    /**
     * Plan an aggregate deletion with its domain event.
     *
     * @template T of DomainEvent
     * @param T $event
     * @return self<T>
     */
    public static function delete(Aggregate $aggregate, Repo $repo, DomainEvent $event): self
    {
        return new self(self::KIND_DELETE, $event, $aggregate, $repo);
    }

    /**
     * Plan a domain event with no aggregate change (e.g. UserLoggedIn).
     *
     * @template T of DomainEvent
     * @param T $event
     * @return self<T>
     */
    public static function emit(DomainEvent $event): self
    {
        return new self(self::KIND_EMIT, $event);
    }

    /**
     * Apply the plan against the unit of work. Only {@link Runner} calls this,
     * inside the transaction it owns — the aggregate write (via the Repo) runs
     * before the event commit so both land in the same transaction.
     *
     * @return Result<TEvent>
     *
     * @internal
     */
    public function apply(UnitOfWork $uow, mixed $command): Result
    {
        switch ($this->kind) {
            case self::KIND_SAVE:
                /** @var Aggregate $agg */
                $agg = $this->aggregate;
                /** @var Repo $repo */
                $repo = $this->repo;
                $repo->persist($agg);
                return $uow->commit($this->event, $command);
            case self::KIND_DELETE:
                /** @var Aggregate $agg */
                $agg = $this->aggregate;
                /** @var Repo $repo */
                $repo = $this->repo;
                $repo->delete($agg);
                return $uow->commit($this->event, $command);
            default: // KIND_EMIT
                return $uow->emitEvent($this->event, $command);
        }
    }
}
