/**
 * UseCase contract.
 *
 * A use case encapsulates one business operation: validation → business rules
 * → build domain event → `unitOfWork.commit(...)`. The only way to return a
 * success is via UnitOfWork, so events are always dispatched.
 */
import { Result } from "./result.js";
import { UseCaseError } from "./errors.js";
/**
 * Base class for use cases that need resource-level authorization.
 *
 * Deny-by-default: subclasses MUST override `authorizeResource` to grant
 * access — either unconditionally (no resource scope) or by checking the
 * command against the caller's scope.
 */
export class SecuredUseCase {
    async execute(command, context) {
        if (!this.authorizeResource(command, context)) {
            return Result.failure(UseCaseError.authorization("RESOURCE_ACCESS_DENIED", "Not authorized to access this resource"));
        }
        return this.doExecute(command, context);
    }
    authorizeResource(_command, _context) {
        return false;
    }
}
