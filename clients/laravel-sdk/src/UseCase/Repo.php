<?php

declare(strict_types=1);

namespace FlowCatalyst\UseCase;

/**
 * Repo — the persistence port a {@link Plan} writes through.
 *
 * Aggregates don't persist themselves; a Repo owns the queries for one
 * aggregate type. Its methods run inside the transaction the unit of work
 * opens for `run` (Laravel's transaction is ambient on the connection), so the
 * aggregate write and the outbox event commit — or roll back — together. The
 * PHP analogue of the Go SDK's `usecasepgx.Persist[A]`.
 *
 * @template TAggregate of Aggregate
 */
interface Repo
{
    /**
     * Insert/upsert the aggregate. Runs on whatever connection/transaction is
     * currently active (the runner opens one around the apply).
     *
     * @param TAggregate $aggregate
     */
    public function persist(Aggregate $aggregate): void;

    /**
     * Delete the aggregate.
     *
     * @param TAggregate $aggregate
     */
    public function delete(Aggregate $aggregate): void;
}
