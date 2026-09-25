<?php

declare(strict_types=1);

namespace FlowCatalyst\UseCase;

/**
 * A use case encapsulates one business operation: validation → business
 * rules → build domain event → `unitOfWork->commit(...)`. Success can
 * only be returned through a `UnitOfWork`.
 *
 * @deprecated Author operations with the envelope instead: implement an
 * {@link Operation} (`validate` / `authorize` / `execute → Plan`) and run it
 * with `Runner::run($uow, $op, $cmd, $ctx)`. The single-method `UseCase`
 * collapses all three phases into `execute` and routes success through the raw
 * `UnitOfWork::commit(...)`; the envelope splits the phases, seals the Plan, and
 * makes the unit of work own the transaction. Kept for one major version.
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
