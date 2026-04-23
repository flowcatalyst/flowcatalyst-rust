<?php

declare(strict_types=1);

namespace FlowCatalyst\UseCase;

/**
 * A use case encapsulates one business operation: validation → business
 * rules → build domain event → `unitOfWork->commit(...)`. Success can
 * only be returned through a `UnitOfWork`.
 *
 * @template TCommand of Command
 * @template TEvent of DomainEvent
 */
interface UseCase
{
    /**
     * @param TCommand $command
     * @return Result<TEvent>
     */
    public function execute(Command $command, ExecutionContext $context): Result;
}
