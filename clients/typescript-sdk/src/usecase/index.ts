/**
 * Use case infrastructure for SDK consumers.
 *
 * The supported authoring pattern is the **envelope**, mirroring the
 * FlowCatalyst platform / Go SDK:
 *
 *   const op: Operation<Cmd, Event> = {
 *     validate(cmd)            { ... },          // optional, pure
 *     authorize(cmd, ctx)      { return null; }, // resource-level, or publicAuthorize
 *     async execute(cmd, ctx)  { ...; return Plan.save(agg, repo, event); },
 *   };
 *   const result = await run(uow, op, cmd, ctx); // applies the Plan in one tx
 *
 * `execute` returns a sealed {@link Plan}; `run` is the only thing that applies
 * it, inside a transaction the {@link OutboxUnitOfWork} owns — so an operation
 * cannot reach the database without also emitting its event atomically.
 */

export { Result, isSuccess, isFailure, type Success, type Failure } from "./result.js";

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

export {
	type Aggregate,
	type UnitOfWork,
	type TxSession,
	type TxRunner,
} from "./unit-of-work.js";

// The use-case envelope (the supported authoring API).
export { type Operation, run, publicAuthorize } from "./operation.js";
export { Plan } from "./plan.js";
export { type Repo } from "./repo.js";

export {
	OutboxUnitOfWork,
	TxScopedOutboxUnitOfWork,
	type OutboxUnitOfWorkConfig,
	type OutboxUnitOfWorkOptions,
} from "./outbox-unit-of-work.js";
