<?php

declare(strict_types=1);

namespace FlowCatalyst\UseCase;

/**
 * Anything with a stable string ID that a UnitOfWork can commit.
 * Kept as a minimal interface so the SDK doesn't force a base class on
 * consumer domain entities.
 */
interface Aggregate
{
    public function id(): string;
}
