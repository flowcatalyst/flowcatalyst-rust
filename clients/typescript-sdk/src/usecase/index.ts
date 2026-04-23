/**
 * Use case infrastructure for SDK consumers.
 *
 * Provides the same pattern used by the FlowCatalyst platform:
 * validation → business rules → DomainEvent → UnitOfWork.commit().
 * The default `OutboxUnitOfWork` dispatches events to the outbox table so
 * fc-outbox-processor forwards them to FlowCatalyst.
 */

export {
	Result,
	isSuccess,
	isFailure,
	type Success,
	type Failure,
	RESULT_SUCCESS_TOKEN,
	type ResultSuccessToken,
} from "./result.js";

export {
	UseCaseError,
	type UseCaseErrorBase,
	type ValidationError,
	type NotFoundError,
	type BusinessRuleViolation,
	type ConcurrencyError,
	type AuthorizationError,
	type InfrastructureError,
} from "./errors.js";

export {
	DomainEvent,
	BaseDomainEvent,
	type DomainEventBase,
} from "./domain-event.js";

export { ExecutionContext } from "./execution-context.js";

export { type Command, type UseCase, SecuredUseCase } from "./use-case.js";

export { type Aggregate, type UnitOfWork } from "./unit-of-work.js";

export {
	OutboxUnitOfWork,
	type OutboxUnitOfWorkConfig,
	type OutboxUnitOfWorkOptions,
} from "./outbox-unit-of-work.js";
