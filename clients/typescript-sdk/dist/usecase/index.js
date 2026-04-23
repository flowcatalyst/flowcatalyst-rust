/**
 * Use case infrastructure for SDK consumers.
 *
 * Provides the same pattern used by the FlowCatalyst platform:
 * validation → business rules → DomainEvent → UnitOfWork.commit().
 * The default `OutboxUnitOfWork` dispatches events to the outbox table so
 * fc-outbox-processor forwards them to FlowCatalyst.
 */
export { Result, isSuccess, isFailure, RESULT_SUCCESS_TOKEN, } from "./result.js";
export { UseCaseError, } from "./errors.js";
export { DomainEvent, BaseDomainEvent, } from "./domain-event.js";
export { ExecutionContext } from "./execution-context.js";
export { SecuredUseCase } from "./use-case.js";
export { OutboxUnitOfWork, } from "./outbox-unit-of-work.js";
