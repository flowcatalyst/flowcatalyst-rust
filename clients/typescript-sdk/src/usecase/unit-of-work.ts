/**
 * UnitOfWork — atomic commit of entity state + domain event (+ optional audit log).
 *
 * Concrete implementations route events to their destination. For this SDK the
 * default is `OutboxUnitOfWork`, which writes events to the local outbox table
 * so the fc-outbox-processor forwards them to the FlowCatalyst platform.
 */

import type { DomainEvent } from "./domain-event.js";
import type { Result } from "./result.js";

export interface Aggregate {
	readonly id: string;
}

export interface UnitOfWork {
	/**
	 * Commit a domain event to the outbox. Optional `persist` runs your entity
	 * writes before the event is emitted — wrap the whole call in your own DB
	 * transaction (using a tx-aware OutboxDriver) for true atomicity.
	 */
	commit<T extends DomainEvent>(
		event: T,
		command: unknown,
		persist?: () => Promise<void>,
	): Promise<Result<T>>;

	/** Commit with an explicit aggregate — convenience for the platform pattern. */
	commitAggregate<T extends DomainEvent>(
		aggregate: Aggregate,
		event: T,
		command: unknown,
		persist?: () => Promise<void>,
	): Promise<Result<T>>;

	/** Commit a delete — same semantics as `commit`, but signals intent. */
	commitDelete<T extends DomainEvent>(
		aggregate: Aggregate,
		event: T,
		command: unknown,
		persist?: () => Promise<void>,
	): Promise<Result<T>>;

	/** Emit an event without an entity change (e.g. UserLoggedIn). */
	emitEvent<T extends DomainEvent>(
		event: T,
		command: unknown,
	): Promise<Result<T>>;
}
