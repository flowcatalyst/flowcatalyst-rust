/**
 * OutboxUnitOfWork — UnitOfWork that dispatches events via the outbox table.
 *
 * `commit()` builds a `CreateEventDto` from the DomainEvent and routes it
 * through `OutboxManager`. The fc-outbox-processor then forwards it to the
 * FlowCatalyst platform. For true atomicity with your entity writes, wrap
 * both the `persist` callback and this commit in a single DB transaction
 * using a tx-aware `OutboxDriver`.
 */

import { OutboxManager } from "../outbox/outbox-manager.js";
import { CreateEventDto } from "../outbox/create-event-dto.js";
import { CreateAuditLogDto } from "../outbox/create-audit-log-dto.js";
import type { OutboxDriver } from "../outbox/types.js";
import { DomainEvent } from "./domain-event.js";
import type { DomainEvent as DomainEventType } from "./domain-event.js";
import { UseCaseError } from "./errors.js";
import {
	isSuccess,
	RESULT_SUCCESS_TOKEN,
	Result,
	type ResultSuccessToken,
} from "./result.js";
import type { Aggregate, UnitOfWork } from "./unit-of-work.js";

export interface OutboxUnitOfWorkOptions {
	/** Emit an audit log alongside every event. Default: false. */
	auditEnabled?: boolean;
	/** Principal ID used in audit logs when the event doesn't carry one. */
	fallbackPrincipalId?: string;
}

export interface OutboxUnitOfWorkConfig {
	outboxManager: OutboxManager;
	options?: OutboxUnitOfWorkOptions;
}

export class OutboxUnitOfWork implements UnitOfWork {
	private readonly outboxManager: OutboxManager;
	private readonly auditEnabled: boolean;
	private readonly fallbackPrincipalId: string;

	constructor(config: OutboxUnitOfWorkConfig) {
		this.outboxManager = config.outboxManager;
		this.auditEnabled = config.options?.auditEnabled ?? false;
		this.fallbackPrincipalId =
			config.options?.fallbackPrincipalId ?? "system";
	}

	/**
	 * Convenience: build from a raw driver + clientId.
	 */
	static fromDriver(
		driver: OutboxDriver,
		clientId: string,
		options?: OutboxUnitOfWorkOptions,
	): OutboxUnitOfWork {
		return new OutboxUnitOfWork({
			outboxManager: new OutboxManager(driver, clientId),
			options,
		});
	}

	async commit<T extends DomainEventType>(
		event: T,
		command: unknown,
		persist?: () => Promise<void>,
	): Promise<Result<T>> {
		return this.doCommit(event, command, persist);
	}

	async commitAggregate<T extends DomainEventType>(
		_aggregate: Aggregate,
		event: T,
		command: unknown,
		persist?: () => Promise<void>,
	): Promise<Result<T>> {
		// The aggregate arg is kept for API parity with the platform UnitOfWork;
		// persistence is the caller's responsibility via `persist`.
		return this.doCommit(event, command, persist);
	}

	async commitDelete<T extends DomainEventType>(
		_aggregate: Aggregate,
		event: T,
		command: unknown,
		persist?: () => Promise<void>,
	): Promise<Result<T>> {
		return this.doCommit(event, command, persist);
	}

	async emitEvent<T extends DomainEventType>(
		event: T,
		command: unknown,
	): Promise<Result<T>> {
		return this.doCommit(event, command);
	}

	/**
	 * Run a callback inside a single application-orchestrated transaction.
	 *
	 * Opens a tx on the underlying driver (via `OutboxDriver.withTransaction`),
	 * builds a {@link TxScopedOutboxUnitOfWork} bound to that tx, and passes
	 * it to the callback. Every outbox write performed against the session —
	 * plus any ad-hoc writes the callback makes via `session.withTx(...)` —
	 * commits atomically when the callback resolves, or rolls back if it
	 * throws or returns a failed `Result`.
	 *
	 * Requires the underlying driver to implement `withTransaction` (the
	 * bundled `PgOutboxDriver` does). Throws at runtime if it doesn't.
	 *
	 * ```ts
	 * const result = await uow.run(async (session) => {
	 *   await session.withTx(async (tx) => {
	 *     await orderRepo.save(order, tx);
	 *   });
	 *   return await session.commit(orderShippedEvent, command);
	 * });
	 * ```
	 *
	 * Mirrors the Rust SDK's `OutboxUnitOfWork::run` and the platform crate's
	 * `PgUnitOfWork::run` so apps and platform follow one orchestration shape.
	 */
	async run<T extends DomainEventType>(
		callback: (session: TxScopedOutboxUnitOfWork) => Promise<Result<T>>,
	): Promise<Result<T>> {
		const driver = this.outboxManager.getDriver();
		if (!driver.withTransaction) {
			return Result.failure<T>(
				UseCaseError.infrastructure(
					"DRIVER_NOT_TX_AWARE",
					"OutboxUnitOfWork.run requires a driver with withTransaction (e.g. PgOutboxDriver)",
				),
			);
		}

		try {
			return await driver.withTransaction(async (tx) => {
				const session = new TxScopedOutboxUnitOfWork({
					outboxManager: this.outboxManager,
					tx,
					auditEnabled: this.auditEnabled,
					fallbackPrincipalId: this.fallbackPrincipalId,
				});
				const result = await callback(session);
				if (!isSuccess(result)) {
					// Throw to trigger rollback; the original Result is preserved
					// via the OutboxRunRollback envelope we catch below.
					throw new OutboxRunRollback(result);
				}
				return result;
			});
		} catch (err) {
			if (err instanceof OutboxRunRollback) {
				return err.result as Result<T>;
			}
			const message = err instanceof Error ? err.message : String(err);
			return Result.failure<T>(
				UseCaseError.infrastructure("COMMIT_FAILED", message, {
					cause: message,
				}),
			);
		}
	}

	private async doCommit<T extends DomainEventType>(
		event: T,
		command: unknown,
		persist?: () => Promise<void>,
	): Promise<Result<T>> {
		try {
			if (persist) {
				await persist();
			}

			await this.outboxManager.createEvent(this.toEventDto(event));

			if (this.auditEnabled) {
				await this.outboxManager.createAuditLog(
					this.toAuditDto(event, command),
				);
			}

			return Result.success<T>(
				RESULT_SUCCESS_TOKEN as ResultSuccessToken,
				event,
			);
		} catch (err) {
			const message = err instanceof Error ? err.message : String(err);
			return Result.failure<T>(
				UseCaseError.infrastructure("COMMIT_FAILED", message, {
					cause: message,
				}),
			);
		}
	}

	private toEventDto<T extends DomainEventType>(event: T): CreateEventDto {
		let dto = CreateEventDto.create(
			event.eventType,
			this.parseData(event.toDataJson()),
		)
			.withSource(event.source)
			.withSubject(event.subject)
			.withCorrelationId(event.correlationId)
			.withMessageGroup(event.messageGroup)
			.withDeduplicationId(`${event.eventType}-${event.eventId}`)
			.withContextData([
				{ key: "principalId", value: event.principalId },
				{ key: "executionId", value: event.executionId },
				{
					key: "aggregateType",
					value: DomainEvent.extractAggregateType(event.subject),
				},
			]);

		if (event.causationId) {
			dto = dto.withCausationId(event.causationId);
		}
		return dto;
	}

	private toAuditDto<T extends DomainEventType>(
		event: T,
		command: unknown,
	): CreateAuditLogDto {
		const entityId = DomainEvent.extractEntityId(event.subject) ?? "";
		const entityType = DomainEvent.extractAggregateType(event.subject);
		const operation = event.eventType.split(":").pop() ?? "unknown";

		const operationData: Record<string, unknown> =
			command && typeof command === "object"
				? (command as Record<string, unknown>)
				: { command };

		return CreateAuditLogDto.create(entityType, entityId, operation)
			.withOperationData(operationData)
			.withPrincipalId(event.principalId || this.fallbackPrincipalId)
			.withCorrelationId(event.correlationId)
			.withSource(event.source)
			.withPerformedAt(event.time);
	}

	private parseData(json: string): Record<string, unknown> {
		try {
			const parsed = JSON.parse(json);
			return typeof parsed === "object" && parsed !== null ? parsed : {};
		} catch {
			return {};
		}
	}
}

// ─── TxScopedOutboxUnitOfWork ────────────────────────────────────────────────

interface TxScopedConfig {
	outboxManager: OutboxManager;
	tx: unknown;
	auditEnabled: boolean;
	fallbackPrincipalId: string;
}

/**
 * UnitOfWork implementation bound to a single, externally-orchestrated
 * transaction opened by {@link OutboxUnitOfWork.run}.
 *
 * Every outbox write performed against this session — events, audit logs —
 * joins the same transaction. The session does NOT commit the tx; the
 * surrounding `run` does (on success) or rolls back (on failure or throw).
 *
 * Use {@link TxScopedOutboxUnitOfWork.withTx} for ad-hoc writes that need
 * to be atomic with the outbox rows (e.g. updating a non-aggregate row).
 */
export class TxScopedOutboxUnitOfWork implements UnitOfWork {
	private readonly outboxManager: OutboxManager;
	private readonly tx: unknown;
	private readonly auditEnabled: boolean;
	private readonly fallbackPrincipalId: string;

	constructor(config: TxScopedConfig) {
		this.outboxManager = config.outboxManager;
		this.tx = config.tx;
		this.auditEnabled = config.auditEnabled;
		this.fallbackPrincipalId = config.fallbackPrincipalId;
	}

	async commit<T extends DomainEventType>(
		event: T,
		command: unknown,
		persist?: () => Promise<void>,
	): Promise<Result<T>> {
		return this.doCommit(event, command, persist);
	}

	async commitAggregate<T extends DomainEventType>(
		_aggregate: Aggregate,
		event: T,
		command: unknown,
		persist?: () => Promise<void>,
	): Promise<Result<T>> {
		return this.doCommit(event, command, persist);
	}

	async commitDelete<T extends DomainEventType>(
		_aggregate: Aggregate,
		event: T,
		command: unknown,
		persist?: () => Promise<void>,
	): Promise<Result<T>> {
		return this.doCommit(event, command, persist);
	}

	async emitEvent<T extends DomainEventType>(
		event: T,
		command: unknown,
	): Promise<Result<T>> {
		return this.doCommit(event, command);
	}

	/**
	 * Invoke the callback with the underlying transaction handle so the
	 * caller can run ad-hoc writes (raw SQL, repository methods, etc.) on
	 * the same tx as the outbox rows. Throws propagate to trigger rollback
	 * of the entire `run` block.
	 */
	async withTx<R>(callback: (tx: unknown) => Promise<R>): Promise<R> {
		return callback(this.tx);
	}

	private async doCommit<T extends DomainEventType>(
		event: T,
		command: unknown,
		persist?: () => Promise<void>,
	): Promise<Result<T>> {
		try {
			if (persist) {
				await persist();
			}

			const eventDto = toEventDtoFor(event);
			await this.outboxManager.createEvent(eventDto, this.tx);

			if (this.auditEnabled) {
				const auditDto = toAuditDtoFor(
					event,
					command,
					this.fallbackPrincipalId,
				);
				await this.outboxManager.createAuditLog(auditDto, this.tx);
			}

			return Result.success<T>(
				RESULT_SUCCESS_TOKEN as ResultSuccessToken,
				event,
			);
		} catch (err) {
			const message = err instanceof Error ? err.message : String(err);
			return Result.failure<T>(
				UseCaseError.infrastructure("COMMIT_FAILED", message, {
					cause: message,
				}),
			);
		}
	}
}

// ─── Internal helpers ────────────────────────────────────────────────────────

/**
 * Thrown by `OutboxUnitOfWork.run` to surface a failed `Result` past the
 * driver's `withTransaction` so the tx rolls back. Caught and unwrapped
 * inside `run` — never escapes the SDK.
 */
class OutboxRunRollback extends Error {
	readonly result: Result<unknown>;

	constructor(result: Result<unknown>) {
		super("OutboxUnitOfWork.run: rolling back transaction");
		this.result = result;
	}
}

const parseDataLocal = (json: string): Record<string, unknown> => {
	try {
		const parsed = JSON.parse(json);
		return typeof parsed === "object" && parsed !== null ? parsed : {};
	} catch {
		return {};
	}
};

const toEventDtoFor = <T extends DomainEventType>(event: T): CreateEventDto => {
	let dto = CreateEventDto.create(event.eventType, parseDataLocal(event.toDataJson()))
		.withSource(event.source)
		.withSubject(event.subject)
		.withCorrelationId(event.correlationId)
		.withMessageGroup(event.messageGroup)
		.withDeduplicationId(`${event.eventType}-${event.eventId}`)
		.withContextData([
			{ key: "principalId", value: event.principalId },
			{ key: "executionId", value: event.executionId },
			{
				key: "aggregateType",
				value: DomainEvent.extractAggregateType(event.subject),
			},
		]);

	if (event.causationId) {
		dto = dto.withCausationId(event.causationId);
	}
	return dto;
};

const toAuditDtoFor = <T extends DomainEventType>(
	event: T,
	command: unknown,
	fallbackPrincipalId: string,
): CreateAuditLogDto => {
	const entityId = DomainEvent.extractEntityId(event.subject) ?? "";
	const entityType = DomainEvent.extractAggregateType(event.subject);
	const operation = event.eventType.split(":").pop() ?? "unknown";

	const operationData: Record<string, unknown> =
		command && typeof command === "object"
			? (command as Record<string, unknown>)
			: { command };

	return CreateAuditLogDto.create(entityType, entityId, operation)
		.withOperationData(operationData)
		.withPrincipalId(event.principalId || fallbackPrincipalId)
		.withCorrelationId(event.correlationId)
		.withSource(event.source)
		.withPerformedAt(event.time);
};
