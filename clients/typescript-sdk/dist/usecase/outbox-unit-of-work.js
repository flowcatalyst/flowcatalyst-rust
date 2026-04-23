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
import { DomainEvent } from "./domain-event.js";
import { UseCaseError } from "./errors.js";
import { RESULT_SUCCESS_TOKEN, Result, } from "./result.js";
export class OutboxUnitOfWork {
    constructor(config) {
        this.outboxManager = config.outboxManager;
        this.auditEnabled = config.options?.auditEnabled ?? false;
        this.fallbackPrincipalId =
            config.options?.fallbackPrincipalId ?? "system";
    }
    /**
     * Convenience: build from a raw driver + clientId.
     */
    static fromDriver(driver, clientId, options) {
        return new OutboxUnitOfWork({
            outboxManager: new OutboxManager(driver, clientId),
            options,
        });
    }
    async commit(event, command, persist) {
        return this.doCommit(event, command, persist);
    }
    async commitAggregate(_aggregate, event, command, persist) {
        // The aggregate arg is kept for API parity with the platform UnitOfWork;
        // persistence is the caller's responsibility via `persist`.
        return this.doCommit(event, command, persist);
    }
    async commitDelete(_aggregate, event, command, persist) {
        return this.doCommit(event, command, persist);
    }
    async emitEvent(event, command) {
        return this.doCommit(event, command);
    }
    async doCommit(event, command, persist) {
        try {
            if (persist) {
                await persist();
            }
            await this.outboxManager.createEvent(this.toEventDto(event));
            if (this.auditEnabled) {
                await this.outboxManager.createAuditLog(this.toAuditDto(event, command));
            }
            return Result.success(RESULT_SUCCESS_TOKEN, event);
        }
        catch (err) {
            const message = err instanceof Error ? err.message : String(err);
            return Result.failure(UseCaseError.infrastructure("COMMIT_FAILED", message, {
                cause: message,
            }));
        }
    }
    toEventDto(event) {
        let dto = CreateEventDto.create(event.eventType, this.parseData(event.toDataJson()))
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
    toAuditDto(event, command) {
        const entityId = DomainEvent.extractEntityId(event.subject) ?? "";
        const entityType = DomainEvent.extractAggregateType(event.subject);
        const operation = event.eventType.split(":").pop() ?? "unknown";
        const operationData = command && typeof command === "object"
            ? command
            : { command };
        return CreateAuditLogDto.create(entityType, entityId, operation)
            .withOperationData(operationData)
            .withPrincipalId(event.principalId || this.fallbackPrincipalId)
            .withCorrelationId(event.correlationId)
            .withSource(event.source)
            .withPerformedAt(event.time);
    }
    parseData(json) {
        try {
            const parsed = JSON.parse(json);
            return typeof parsed === "object" && parsed !== null ? parsed : {};
        }
        catch {
            return {};
        }
    }
}
