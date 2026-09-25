/**
 * UnitOfWork — atomic commit of entity state + domain event (+ optional audit log).
 *
 * Concrete implementations route events to their destination. For this SDK the
 * default is `OutboxUnitOfWork`, which writes events to the local outbox table
 * so the outbox poller forwards them to the FlowCatalyst platform.
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

/**
 * TxSession is a {@link UnitOfWork} bound to a single open transaction, plus
 * `withTx` for ad-hoc writes on that same transaction. It is what an
 * orchestrated `run` (see {@link TxRunner}) hands to its callback;
 * `TxScopedOutboxUnitOfWork` implements it. The use-case envelope applies a
 * `Plan` against a TxSession so the aggregate write and the event commit
 * share one transaction.
 */
export interface TxSession extends UnitOfWork {
	/** Run a callback with the bound transaction handle (raw SQL, repos, …). */
	withTx<R>(callback: (tx: unknown) => Promise<R>): Promise<R>;
}

/**
 * TxRunner owns a transaction: it opens one, hands a {@link TxSession} to the
 * callback, and commits when the callback resolves with a success `Result` or
 * rolls back on a failed `Result`/throw. `OutboxUnitOfWork` implements it. The
 * envelope's `run` drives an Operation and applies its Plan through this.
 */
export interface TxRunner {
	run<T extends DomainEvent>(
		callback: (session: TxSession) => Promise<Result<T>>,
	): Promise<Result<T>>;
}
