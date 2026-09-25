/**
 * `OutboxUnitOfWork.layer` — Effect Layer for the `UnitOfWork` Tag, backed by
 * the existing `OutboxManager`. Same semantics as the neverthrow
 * `OutboxUnitOfWork` in `src/usecase/outbox-unit-of-work.ts`; the difference
 * is that success is produced as a `Sealed<E>`.
 */

import { Effect, Layer } from "effect";
import { CreateAuditLogDto } from "../../outbox/create-audit-log-dto.js";
import { auditMaskedFieldsOf } from "../../outbox/audit-redaction.js";
import { CreateEventDto } from "../../outbox/create-event-dto.js";
import { OutboxManager } from "../../outbox/outbox-manager.js";
import type { OutboxDriver } from "../../outbox/types.js";
import {
	DomainEvent as DomainEventUtils,
	type DomainEvent,
} from "../../usecase/domain-event.js";
import { InfrastructureError } from "./errors.js";
import { seal } from "./seal.js";
import { UnitOfWork } from "./unit-of-work.js";

export interface OutboxUnitOfWorkOptions {
	/** Emit an audit log alongside every event. Default: false. */
	readonly auditEnabled?: boolean;
	/** Principal ID used in audit logs when the event doesn't carry one. */
	readonly fallbackPrincipalId?: string;
	/**
	 * FlowCatalyst application + client codes the emitted events / audit logs
	 * belong to (client-centric platform; resolved at ingest). Audit carries
	 * both; events carry the client code. Omitted when unset.
	 */
	readonly applicationCode?: string;
	readonly clientCode?: string;
}

const parseJson = (json: string): Record<string, unknown> => {
	try {
		const v: unknown = JSON.parse(json);
		return typeof v === "object" && v !== null
			? (v as Record<string, unknown>)
			: {};
	} catch {
		return {};
	}
};

const toEventDto = <E extends DomainEvent>(
	event: E,
	clientCode?: string,
): CreateEventDto => {
	let dto = CreateEventDto.create(event.eventType, parseJson(event.toDataJson()))
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
				value: DomainEventUtils.extractAggregateType(event.subject),
			},
		]);
	if (event.causationId) dto = dto.withCausationId(event.causationId);
	if (clientCode) dto = dto.withClientCode(clientCode);
	return dto;
};

const toAuditDto = <E extends DomainEvent>(
	event: E,
	command: unknown,
	fallbackPrincipalId: string,
	applicationCode?: string,
	clientCode?: string,
): CreateAuditLogDto => {
	const entityId = DomainEventUtils.extractEntityId(event.subject) ?? "";
	const entityType = DomainEventUtils.extractAggregateType(event.subject);
	const operation = event.eventType.split(":").pop() ?? "unknown";
	const operationData: Record<string, unknown> =
		command && typeof command === "object"
			? (command as Record<string, unknown>)
			: { command };
	let dto = CreateAuditLogDto.create(entityType, entityId, operation)
		.withOperationData(operationData, auditMaskedFieldsOf(command))
		.withPrincipalId(event.principalId || fallbackPrincipalId)
		.withCorrelationId(event.correlationId)
		.withSource(event.source)
		.withPerformedAt(event.time);
	if (applicationCode) dto = dto.withApplicationCode(applicationCode);
	if (clientCode) dto = dto.withClientCode(clientCode);
	return dto;
};

/**
 * Build a `UnitOfWork` Layer backed by an existing `OutboxManager`.
 *
 * @example
 * ```ts
 * import { OutboxManager } from "@flowcatalyst/sdk";
 * import { OutboxUnitOfWork } from "@flowcatalyst/sdk/effect/usecase";
 *
 * const outboxManager = new OutboxManager(driver, clientId);
 * const Live = OutboxUnitOfWork.layer(outboxManager, { auditEnabled: true });
 * ```
 */
export const layer = (
	outboxManager: OutboxManager,
	options?: OutboxUnitOfWorkOptions,
): Layer.Layer<UnitOfWork> => {
	const auditEnabled = options?.auditEnabled ?? false;
	const fallbackPrincipalId = options?.fallbackPrincipalId ?? "system";
	const applicationCode = options?.applicationCode;
	const clientCode = options?.clientCode;

	const doCommit = <E extends DomainEvent>(
		event: E,
		command: unknown,
		persist?: () => Promise<void>,
	) =>
		Effect.tryPromise({
			try: async () => {
				if (persist) await persist();
				await outboxManager.createEvent(toEventDto(event, clientCode));
				if (auditEnabled) {
					await outboxManager.createAuditLog(
						toAuditDto(
							event,
							command,
							fallbackPrincipalId,
							applicationCode,
							clientCode,
						),
					);
				}
				return seal(event);
			},
			catch: (e) =>
				new InfrastructureError({
					code: "COMMIT_FAILED",
					message: e instanceof Error ? e.message : String(e),
				}),
		});

	return Layer.succeed(UnitOfWork, {
		commit: doCommit,
		commitDelete: (_aggregate, event, command, persist) =>
			doCommit(event, command, persist),
		emitEvent: (event, command) => doCommit(event, command),
	});
};

/** Build a Layer from a raw driver + `clientId`. */
export const layerFromDriver = (
	driver: OutboxDriver,
	clientId: string,
	options?: OutboxUnitOfWorkOptions,
): Layer.Layer<UnitOfWork> =>
	layer(new OutboxManager(driver, clientId), options);
