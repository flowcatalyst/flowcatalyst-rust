/**
 * Plan<E> — a pending change + the domain event it produces, built by an
 * {@link Operation}'s `execute` phase but NOT yet committed. `run` is the only
 * thing that applies a Plan, and it does so inside one transaction (aggregate
 * write + event + optional audit, atomically).
 *
 * Plan is sealed: its discriminant key is a module-private `unique symbol`
 * (the same airtight technique as `src/effect/usecase/seal.ts`), and the only
 * constructors are {@link Plan.save} / {@link Plan.delete} / {@link Plan.emit}.
 * External code can speak the *type* `Plan<E>` but cannot construct one — so an
 * operation cannot reach the database except by returning a Plan and letting
 * `run` apply it. The TypeScript analogue of the Go SDK's sealed `Plan[E]`.
 */

import { Result } from "./result.js";
import type { DomainEvent } from "./domain-event.js";
import type { Repo } from "./repo.js";
import type { Aggregate, TxSession } from "./unit-of-work.js";

const PlanKind: unique symbol = Symbol("flowcatalyst.usecase.plan");
type PlanKind = typeof PlanKind;

interface SavePlan<E extends DomainEvent> {
	readonly [PlanKind]: "save";
	readonly aggregate: Aggregate;
	readonly repo: Repo<Aggregate>;
	readonly event: E;
}

interface DeletePlan<E extends DomainEvent> {
	readonly [PlanKind]: "delete";
	readonly aggregate: Aggregate;
	readonly repo: Repo<Aggregate>;
	readonly event: E;
}

interface EmitPlan<E extends DomainEvent> {
	readonly [PlanKind]: "emit";
	readonly event: E;
}

export type Plan<E extends DomainEvent> =
	| SavePlan<E>
	| DeletePlan<E>
	| EmitPlan<E>;

export const Plan = {
	/** Plan an aggregate upsert with its domain event (create / update). */
	save<A extends Aggregate, E extends DomainEvent>(
		aggregate: A,
		repo: Repo<A>,
		event: E,
	): Plan<E> {
		return {
			[PlanKind]: "save",
			aggregate,
			repo: repo as unknown as Repo<Aggregate>,
			event,
		};
	},

	/** Plan an aggregate deletion with its domain event. */
	delete<A extends Aggregate, E extends DomainEvent>(
		aggregate: A,
		repo: Repo<A>,
		event: E,
	): Plan<E> {
		return {
			[PlanKind]: "delete",
			aggregate,
			repo: repo as unknown as Repo<Aggregate>,
			event,
		};
	},

	/** Plan a domain event with no aggregate change (e.g. UserLoggedIn). */
	emit<E extends DomainEvent>(event: E): Plan<E> {
		return { [PlanKind]: "emit", event };
	},
};

/** True when `x` is a Plan (used by `run` to tell a Plan from a returned error). */
export function isPlan(x: unknown): x is Plan<DomainEvent> {
	return typeof x === "object" && x !== null && PlanKind in x;
}

/**
 * @internal — apply a Plan against the transaction-bound session. Only `run`
 * calls this, inside the transaction the TxRunner owns. Lives here so the
 * `PlanKind` discriminant never has to be exported.
 */
export async function applyPlan<E extends DomainEvent>(
	session: TxSession,
	plan: Plan<E>,
	command: unknown,
): Promise<Result<E>> {
	switch (plan[PlanKind]) {
		case "save":
			return session.commit(plan.event, command, () =>
				session.withTx((tx) => plan.repo.persist(plan.aggregate, tx)),
			);
		case "delete":
			return session.commit(plan.event, command, () =>
				session.withTx((tx) => plan.repo.delete(plan.aggregate, tx)),
			);
		case "emit":
			return session.emitEvent(plan.event, command);
	}
}
