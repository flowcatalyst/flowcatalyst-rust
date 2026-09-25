/**
 * Operation<C, E> — one business operation expressed as named phases, and
 * `run`, the driver that executes them and atomically applies the resulting
 * {@link Plan}. The TypeScript port of the Go SDK's `usecaseop.Operation` /
 * `usecaseop.Run`.
 *
 *   validate  → command shape (pure, no I/O)
 *   authorize → resource-level access (use `publicAuthorize` to declare open)
 *   execute   → load + invariants → return a Plan (or a UseCaseError)
 *   ───────── run then applies the Plan in one transaction (aggregate + event + audit)
 *
 * An operation cannot reach the database except by returning a Plan; `run` is
 * the only thing that applies one, and it does so through a {@link TxRunner}
 * that owns the transaction. So "aggregate written ⇒ event written, atomically"
 * holds by construction.
 */

import { Result } from "./result.js";
import { UseCaseError } from "./errors.js";
import { applyPlan, isPlan, type Plan } from "./plan.js";
import type { Command } from "./use-case.js";
import type { DomainEvent } from "./domain-event.js";
import type { ExecutionContext } from "./execution-context.js";
import type { TxRunner } from "./unit-of-work.js";

export interface Operation<TCommand extends Command, TEvent extends DomainEvent> {
	/** Identifies the operation in diagnostics. Optional but recommended. */
	readonly name?: string;

	/**
	 * Check command shape — presence, format, length, patterns; anything that
	 * does not require loading data. Return a validation-kind {@link UseCaseError}
	 * on failure, or `null`/`undefined` to pass. Optional.
	 */
	validate?(command: TCommand): UseCaseError | null | undefined;

	/**
	 * Resource-level access check (ownership, scope, state). REQUIRED — set it
	 * to a real check, or to {@link publicAuthorize} to declare the operation
	 * intentionally open. Coarse RBAC stays at the transport layer.
	 */
	authorize(
		command: TCommand,
		context: ExecutionContext,
	): UseCaseError | null | undefined;

	/**
	 * Run invariant checks (load aggregates, apply business rules), build the
	 * domain event, and return the {@link Plan} describing the change. Returning
	 * a Plan is the only way to reach the database. Return a {@link UseCaseError}
	 * (or throw one) to fail without committing.
	 */
	execute(
		command: TCommand,
		context: ExecutionContext,
	): Promise<Plan<TEvent> | UseCaseError>;
}

/**
 * The explicit `authorize` value for operations that are intentionally open —
 * no resource-level check beyond whatever the transport already enforced.
 * Prefer this over an `authorize` that just returns null: "deliberately open"
 * should be a visible decision.
 */
export function publicAuthorize(): null {
	return null;
}

/**
 * Run op's phases in order — validate → authorize → execute — short-circuiting
 * on the first error, then apply the returned {@link Plan} in one transaction
 * (aggregate change + domain event + audit log) and return the committed event.
 *
 * `uow` is the transaction owner (e.g. `OutboxUnitOfWork`); `run` never reaches
 * the database itself — it only drives the phases and hands the Plan to
 * `uow.run`, which owns commit/rollback.
 */
export async function run<TCommand extends Command, TEvent extends DomainEvent>(
	uow: TxRunner,
	op: Operation<TCommand, TEvent>,
	command: TCommand,
	context: ExecutionContext,
): Promise<Result<TEvent>> {
	if (op.validate) {
		const err = op.validate(command);
		if (err) return Result.failure<TEvent>(err);
	}

	const authErr = op.authorize(command, context);
	if (authErr) return Result.failure<TEvent>(authErr);

	let outcome: Plan<TEvent> | UseCaseError;
	try {
		outcome = await op.execute(command, context);
	} catch (err) {
		if (UseCaseError.isUseCaseError(err)) return Result.failure<TEvent>(err);
		const message = err instanceof Error ? err.message : String(err);
		return Result.failure<TEvent>(
			UseCaseError.infrastructure("EXECUTE_FAILED", message, { cause: message }),
		);
	}

	if (!isPlan(outcome)) {
		// execute returned a UseCaseError (the invariant-failure path).
		return Result.failure<TEvent>(outcome);
	}

	return uow.run<TEvent>((session) => applyPlan(session, outcome as Plan<TEvent>, command));
}
