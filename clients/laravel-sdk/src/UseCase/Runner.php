<?php

declare(strict_types=1);

namespace FlowCatalyst\UseCase;

/**
 * Runner — drives an {@link Operation}'s phases and atomically applies the
 * resulting {@link Plan}. The PHP port of the Go SDK's `usecaseop.Run`.
 *
 * `run` executes validate → authorize → execute, short-circuiting on the first
 * error, then opens the transaction the unit of work owns and applies the Plan
 * (aggregate change + domain event + audit log) inside it. A failed Result —
 * or any throw — rolls the whole transaction back; the failure is returned, not
 * swallowed. Handlers always call `run`; they never touch the raw
 * `UnitOfWork::commit(...)`.
 */
final class Runner
{
    /**
     * @template TCommand of Command
     * @template TEvent of DomainEvent
     * @param Operation<TCommand, TEvent> $operation
     * @param TCommand $command
     * @return Result<TEvent>
     */
    public static function run(
        UnitOfWork $unitOfWork,
        Operation $operation,
        Command $command,
        ExecutionContext $context,
    ): Result {
        $validationError = $operation->validate($command);
        if ($validationError !== null) {
            return Result::failure($validationError);
        }

        $authorizationError = $operation->authorize($command, $context);
        if ($authorizationError !== null) {
            return Result::failure($authorizationError);
        }

        try {
            $outcome = $operation->execute($command, $context);
        } catch (\Throwable $err) {
            return Result::failure(UseCaseError::infrastructure(
                'EXECUTE_FAILED',
                $err->getMessage(),
                ['cause' => $err::class],
            ));
        }

        if ($outcome instanceof UseCaseError) {
            return Result::failure($outcome);
        }

        // $outcome is a Plan — apply it in the unit of work's transaction.
        try {
            /** @var Result<TEvent> $result */
            $result = $unitOfWork->transaction(function () use ($unitOfWork, $outcome, $command): Result {
                $applied = $outcome->apply($unitOfWork, $command);
                if ($applied->isFailure()) {
                    // Throw so the transaction rolls back; the failed Result is
                    // carried out via RollbackSignal and returned below.
                    throw new RollbackSignal($applied);
                }
                return $applied;
            });
            return $result;
        } catch (RollbackSignal $signal) {
            return $signal->result;
        } catch (\Throwable $err) {
            return Result::failure(UseCaseError::infrastructure(
                'COMMIT_FAILED',
                $err->getMessage(),
                ['cause' => $err::class],
            ));
        }
    }
}
