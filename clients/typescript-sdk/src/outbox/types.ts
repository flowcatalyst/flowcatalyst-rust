/**
 * Outbox types for transactional outbox pattern.
 *
 * Schema must match the Java outbox-processor's expected format.
 * The processor reads from these tables and manages status transitions.
 */

/** Message types supported by the outbox. */
export type MessageType = "EVENT" | "DISPATCH_JOB" | "AUDIT_LOG";

/**
 * Outbox status codes matching the Java outbox-processor.
 *
 * The processor uses SMALLINT status codes, NOT strings.
 * Only PENDING (0) is written by the SDK; all others are set by the processor.
 */
export const OutboxStatus = {
	/** Waiting to be processed. */
	PENDING: 0,
	/** Successfully sent to FlowCatalyst. */
	SUCCESS: 1,
	/** API returned 400 Bad Request (permanent failure). */
	BAD_REQUEST: 2,
	/** API returned 500 Internal Server Error (retryable). */
	INTERNAL_ERROR: 3,
	/** API returned 401 Unauthorized (retryable). */
	UNAUTHORIZED: 4,
	/** API returned 403 Forbidden (permanent failure). */
	FORBIDDEN: 5,
	/** API returned 502/503/504 Gateway Error (retryable). */
	GATEWAY_ERROR: 6,
	/** Currently being processed - crash recovery marker. */
	IN_PROGRESS: 9,
} as const;

export type OutboxStatusCode = (typeof OutboxStatus)[keyof typeof OutboxStatus];

/** An outbox message record to be persisted by the driver. */
export interface OutboxMessage {
	id: string;
	type: MessageType;
	message_group: string | null;
	payload: string;
	status: number;
	created_at: string;
	updated_at: string;
	/** SDK-specific: client identifier for multi-tenant routing. */
	client_id: string;
	/** SDK-specific: byte size of payload. */
	payload_size: number;
	/** SDK-specific: optional headers. */
	headers: Record<string, string> | null;
}

/**
 * Driver interface for outbox persistence.
 *
 * Implementations write outbox rows. To make outbox writes atomic with the
 * caller's business writes, pass the same transaction handle (opaque to the
 * SDK) to both: your repository's persist call and the driver's insert call.
 *
 * - `insert` / `insertBatch` accept an optional `tx` handle. If omitted, the
 *   driver writes against its default executor (typically a pool). If
 *   present, the driver writes against the caller-supplied transaction so
 *   the row is part of the same tx as the business writes.
 * - `withTransaction` is optional; implementations that support it enable
 *   `OutboxUnitOfWork.run(callback)`, which opens a tx, runs the callback
 *   against a scoped UoW, then commits or rolls back based on the result.
 *
 * The bundled [`PgOutboxDriver`](./drivers/pg-outbox-driver.js) implements
 * both methods against `node-postgres`-compatible pools and clients (including
 * Drizzle's underlying client). Most consumers should use it directly.
 */
export interface OutboxDriver {
	/**
	 * Insert a single message into the outbox.
	 *
	 * If `tx` is provided, the write joins that transaction. Otherwise the
	 * driver writes via its default executor.
	 */
	insert(message: OutboxMessage, tx?: unknown): Promise<void>;

	/**
	 * Insert multiple messages into the outbox (batch).
	 *
	 * If `tx` is provided, all rows join that transaction. Otherwise the
	 * driver opens its own short-lived transaction so the batch is atomic.
	 */
	insertBatch(messages: OutboxMessage[], tx?: unknown): Promise<void>;

	/**
	 * Open a transaction, run the callback against the tx handle, and commit
	 * (or roll back on throw). Required for `OutboxUnitOfWork.run` — drivers
	 * that don't implement this can still be used via the non-orchestrated
	 * `commit` / `commitAggregate` / `emitEvent` methods.
	 */
	withTransaction?<T>(callback: (tx: unknown) => Promise<T>): Promise<T>;
}
