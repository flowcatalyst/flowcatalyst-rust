<?php

declare(strict_types=1);

namespace FlowCatalyst\UseCase;

/**
 * Base class for use cases with resource-level authorization.
 *
 * `authorizeResource()` is **deny-by-default** — subclasses MUST override it
 * and return either `true` unconditionally (no per-resource scope) or a
 * decision based on the caller's scope.
 *
 * @deprecated Use the envelope: an {@link Operation}'s `authorize` phase is the
 * direct replacement for `authorizeResource` (return a {@link UseCaseError} to
 * deny, `null` to allow). Run it with `Runner::run(...)`.
 *
 * @template TCommand of Command
 * @template TEvent of DomainEvent
 * @implements UseCase<TCommand, TEvent>
 */
abstract class SecuredUseCase implements UseCase
{
    /**
     * @param TCommand $command
     * @return Result<TEvent>
     */
    public function execute(Command $command, ExecutionContext $context): Result
    {
        if (! $this->authorizeResource($command, $context)) {
            return Result::failure(UseCaseError::authorization(
                'RESOURCE_ACCESS_DENIED',
                'Not authorized to access this resource',
            ));
        }
        return $this->doExecute($command, $context);
    }

    /**
     * Override to grant access. Default denies.
     *
     * @param TCommand $command
     */
    protected function authorizeResource(Command $command, ExecutionContext $context): bool
    {
        return false;
    }

    /**
     * @param TCommand $command
     * @return Result<TEvent>
     */
    abstract protected function doExecute(Command $command, ExecutionContext $context): Result;
}
