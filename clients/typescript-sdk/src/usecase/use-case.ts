/**
 * UseCase contract.
 *
 * A use case encapsulates one business operation: validation → business rules
 * → build domain event → `unitOfWork.commit(...)`. The only way to return a
 * success is via UnitOfWork, so events are always dispatched.
 */

import { Result } from "./result.js";
import { UseCaseError } from "./errors.js";
import type { DomainEvent } from "./domain-event.js";
import type { ExecutionContext } from "./execution-context.js";

export interface Command {
	// Marker interface — concrete commands extend with their fields.
}

export interface UseCase<TCommand extends Command, TEvent extends DomainEvent> {
	execute(
		command: TCommand,
		context: ExecutionContext,
	): Promise<Result<TEvent>>;
}

/**
 * Base class for use cases that need resource-level authorization.
 *
 * Deny-by-default: subclasses MUST override `authorizeResource` to grant
 * access — either unconditionally (no resource scope) or by checking the
 * command against the caller's scope.
 */
export abstract class SecuredUseCase<
	TCommand extends Command,
	TEvent extends DomainEvent,
> implements UseCase<TCommand, TEvent>
{
	async execute(
		command: TCommand,
		context: ExecutionContext,
	): Promise<Result<TEvent>> {
		if (!this.authorizeResource(command, context)) {
			return Result.failure(
				UseCaseError.authorization(
					"RESOURCE_ACCESS_DENIED",
					"Not authorized to access this resource",
				),
			);
		}
		return this.doExecute(command, context);
	}

	authorizeResource(_command: TCommand, _context: ExecutionContext): boolean {
		return false;
	}

	abstract doExecute(
		command: TCommand,
		context: ExecutionContext,
	): Promise<Result<TEvent>>;
}
