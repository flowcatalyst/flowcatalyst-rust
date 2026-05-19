/**
 * `PgOutboxDriver` — outbox driver for node-postgres-compatible clients.
 *
 * Works with anything that exposes the minimal `query(text, params)` shape:
 * `pg.Pool`, `pg.PoolClient`, and Drizzle's underlying client (accessible via
 * `db.$client` on node-postgres-backed Drizzle instances). No hard dep on
 * `pg` — duck-typed via {@link PgQueryable} so the SDK doesn't pull node-
 * postgres into installs that don't need it.
 *
 * Transactional outbox usage:
 *
 * ```ts
 * import { Pool } from "pg";
 * import {
 *   OutboxManager,
 *   OutboxUnitOfWork,
 *   PgOutboxDriver,
 * } from "@flowcatalyst/sdk";
 *
 * const pool = new Pool({ connectionString });
 * const driver = new PgOutboxDriver(pool);
 * const outbox = new OutboxManager(driver, "clt_0HZXEQ5Y8JY5Z");
 * const uow = OutboxUnitOfWork.fromDriver(driver, "clt_0HZXEQ5Y8JY5Z");
 *
 * await uow.run(async (session) => {
 *   // Every write inside the callback joins the same transaction:
 *   await session.withTx(async (tx) => {
 *     await tx.query("UPDATE orders SET status='shipped' WHERE id=$1", [orderId]);
 *   });
 *   return await session.commit(orderShippedEvent, command);
 * });
 * ```
 */

import type { OutboxDriver, OutboxMessage } from "../types.js";

/**
 * Minimal `pg`-compatible query interface. Both `pg.Pool` and `pg.PoolClient`
 * (and Drizzle's underlying client) satisfy this shape — no explicit `pg`
 * dependency required.
 */
export interface PgQueryable {
	query(
		text: string,
		params?: ReadonlyArray<unknown>,
	): Promise<{ rowCount?: number | null } | unknown>;
}

/**
 * `PgQueryable` augmented with the transaction-checkout method exposed by
 * `pg.Pool`. Only `Pool` (not `PoolClient`) supports `connect`, so
 * `withTransaction` is only available when the driver was constructed with
 * a pool-like executor.
 */
export interface PgPoolLike extends PgQueryable {
	connect(): Promise<PgPoolClientLike>;
}

/** Minimal shape of `pg.PoolClient` used inside `withTransaction`. */
export interface PgPoolClientLike extends PgQueryable {
	release(err?: Error | boolean): void;
}

const INSERT_SQL = `
INSERT INTO outbox_messages (
  id, type, message_group, payload, status, created_at, updated_at,
  client_id, payload_size, headers
)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
`;

const isPool = (executor: PgQueryable): executor is PgPoolLike =>
	typeof (executor as { connect?: unknown }).connect === "function";

const isPoolClient = (tx: unknown): tx is PgPoolClientLike =>
	typeof tx === "object" &&
	tx !== null &&
	typeof (tx as { query?: unknown }).query === "function";

const buildParams = (message: OutboxMessage): ReadonlyArray<unknown> => [
	message.id,
	message.type,
	message.message_group,
	message.payload,
	message.status,
	message.created_at,
	message.updated_at,
	message.client_id,
	message.payload_size,
	message.headers ? JSON.stringify(message.headers) : null,
];

export class PgOutboxDriver implements OutboxDriver {
	private readonly executor: PgQueryable;

	constructor(executor: PgQueryable) {
		this.executor = executor;
	}

	/**
	 * Insert a single outbox row.
	 *
	 * If `tx` is a checked-out `pg.PoolClient` (typically obtained inside
	 * `withTransaction`), the row joins that transaction. Otherwise the row
	 * is written against the default executor passed to the constructor.
	 */
	async insert(message: OutboxMessage, tx?: unknown): Promise<void> {
		const client: PgQueryable = isPoolClient(tx) ? tx : this.executor;
		await client.query(INSERT_SQL, buildParams(message));
	}

	/**
	 * Insert multiple outbox rows.
	 *
	 * If `tx` is provided, all rows join that transaction. Otherwise the
	 * driver checks out a client from its pool, opens a short-lived
	 * transaction so the batch is atomic, and releases the client.
	 *
	 * Note: this is N round trips. If you have a large batch, prefer
	 * passing your own `tx` from inside `withTransaction` and let the
	 * surrounding work decide when to commit.
	 */
	async insertBatch(messages: OutboxMessage[], tx?: unknown): Promise<void> {
		if (messages.length === 0) return;

		if (isPoolClient(tx)) {
			for (const message of messages) {
				await tx.query(INSERT_SQL, buildParams(message));
			}
			return;
		}

		if (!isPool(this.executor)) {
			// Bare PgQueryable (no `connect`) — caller must supply a tx or
			// accept a per-row write with no batch-level atomicity.
			for (const message of messages) {
				await this.executor.query(INSERT_SQL, buildParams(message));
			}
			return;
		}

		const client = await this.executor.connect();
		try {
			await client.query("BEGIN");
			for (const message of messages) {
				await client.query(INSERT_SQL, buildParams(message));
			}
			await client.query("COMMIT");
		} catch (err) {
			try {
				await client.query("ROLLBACK");
			} catch {
				/* ignore rollback errors */
			}
			throw err;
		} finally {
			client.release();
		}
	}

	/**
	 * Open a transaction on the pool, run the callback against the checked-
	 * out client, and commit (or roll back on throw). Used by
	 * `OutboxUnitOfWork.run` to give the caller a single tx that spans both
	 * business writes and outbox writes.
	 *
	 * Only available when the driver was constructed with a pool-like
	 * executor (one that exposes `connect()`). With a bare `PgQueryable`
	 * (e.g. a single `pg.Client`), the caller must manage the transaction
	 * boundary themselves and pass the client as `tx` to insert calls.
	 */
	async withTransaction<T>(
		callback: (tx: PgPoolClientLike) => Promise<T>,
	): Promise<T> {
		if (!isPool(this.executor)) {
			throw new Error(
				"PgOutboxDriver.withTransaction requires a pool-like executor with `connect()`. Construct the driver with a `pg.Pool` to enable orchestrated transactions.",
			);
		}

		const client = await this.executor.connect();
		try {
			await client.query("BEGIN");
			const result = await callback(client);
			await client.query("COMMIT");
			return result;
		} catch (err) {
			try {
				await client.query("ROLLBACK");
			} catch {
				/* ignore rollback errors */
			}
			throw err;
		} finally {
			client.release();
		}
	}
}
