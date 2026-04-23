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
