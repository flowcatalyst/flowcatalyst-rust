/**
 * UseCase contract.
 *
 * A use case encapsulates one business operation: validation → business rules
 * → build domain event → `unitOfWork.commit(...)`. The only way to return a
 * success is via UnitOfWork, so events are always dispatched.
 */
import { Result } from "./result.js";
import type { DomainEvent } from "./domain-event.js";
import type { ExecutionContext } from "./execution-context.js";
export interface Command {
}
export interface UseCase<TCommand extends Command, TEvent extends DomainEvent> {
    execute(command: TCommand, context: ExecutionContext): Promise<Result<TEvent>>;
}
/**
 * Base class for use cases that need resource-level authorization.
 *
 * Deny-by-default: subclasses MUST override `authorizeResource` to grant
 * access — either unconditionally (no resource scope) or by checking the
 * command against the caller's scope.
 */
export declare abstract class SecuredUseCase<TCommand extends Command, TEvent extends DomainEvent> implements UseCase<TCommand, TEvent> {
    execute(command: TCommand, context: ExecutionContext): Promise<Result<TEvent>>;
    authorizeResource(_command: TCommand, _context: ExecutionContext): boolean;
    abstract doExecute(command: TCommand, context: ExecutionContext): Promise<Result<TEvent>>;
}
//# sourceMappingURL=use-case.d.ts.map