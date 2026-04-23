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
import type { OutboxDriver } from "../outbox/types.js";
import type { DomainEvent as DomainEventType } from "./domain-event.js";
import { Result } from "./result.js";
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
export declare class OutboxUnitOfWork implements UnitOfWork {
    private readonly outboxManager;
    private readonly auditEnabled;
    private readonly fallbackPrincipalId;
    constructor(config: OutboxUnitOfWorkConfig);
    /**
     * Convenience: build from a raw driver + clientId.
     */
    static fromDriver(driver: OutboxDriver, clientId: string, options?: OutboxUnitOfWorkOptions): OutboxUnitOfWork;
    commit<T extends DomainEventType>(event: T, command: unknown, persist?: () => Promise<void>): Promise<Result<T>>;
    commitAggregate<T extends DomainEventType>(_aggregate: Aggregate, event: T, command: unknown, persist?: () => Promise<void>): Promise<Result<T>>;
    commitDelete<T extends DomainEventType>(_aggregate: Aggregate, event: T, command: unknown, persist?: () => Promise<void>): Promise<Result<T>>;
    emitEvent<T extends DomainEventType>(event: T, command: unknown): Promise<Result<T>>;
    private doCommit;
    private toEventDto;
    private toAuditDto;
    private parseData;
}
//# sourceMappingURL=outbox-unit-of-work.d.ts.map