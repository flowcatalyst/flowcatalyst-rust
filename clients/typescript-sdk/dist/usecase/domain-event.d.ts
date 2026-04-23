/**
 * DomainEvent — CloudEvents-style facts about what happened.
 *
 * Concrete events extend `BaseDomainEvent<TData>` — they supply the
 * type/source/subject/messageGroup base fields plus the data payload;
 * `BaseDomainEvent` populates eventId/correlation/causation/principal from
 * an `ExecutionContext`.
 */
import type { ExecutionContext } from "./execution-context.js";
export interface DomainEvent {
    readonly eventId: string;
    readonly eventType: string;
    readonly specVersion: string;
    readonly source: string;
    readonly subject: string;
    readonly time: Date;
    readonly executionId: string;
    readonly correlationId: string;
    readonly causationId: string | null;
    readonly principalId: string;
    readonly messageGroup: string;
    /** JSON-serialized data payload (domain-specific fields). */
    toDataJson(): string;
}
export interface DomainEventBase {
    eventType: string;
    specVersion: string;
    source: string;
    subject: string;
    messageGroup: string;
}
export declare const DomainEvent: {
    generateId(): string;
    subject(domain: string, aggregate: string, id: string): string;
    messageGroup(domain: string, aggregate: string, id: string): string;
    eventType(app: string, domain: string, aggregate: string, action: string): string;
    /** "platform.eventtype.123" → "Eventtype" */
    extractAggregateType(subject: string): string;
    /** "platform.eventtype.123" → "123" */
    extractEntityId(subject: string): string | null;
};
/**
 * Base class for concrete domain events.
 *
 * @example
 * ```ts
 * class OrderShipped extends BaseDomainEvent<{ orderId: string; tracking: string }> {
 *   constructor(ctx: ExecutionContext, data: { orderId: string; tracking: string }) {
 *     super(
 *       {
 *         eventType: "shop:orders:order:shipped",
 *         specVersion: "1.0",
 *         source: "shop:orders",
 *         subject: DomainEvent.subject("orders", "order", data.orderId),
 *         messageGroup: DomainEvent.messageGroup("orders", "order", data.orderId),
 *       },
 *       ctx,
 *       data,
 *     );
 *   }
 * }
 * ```
 */
export declare abstract class BaseDomainEvent<TData extends object> implements DomainEvent {
    readonly eventId: string;
    readonly eventType: string;
    readonly specVersion: string;
    readonly source: string;
    readonly subject: string;
    readonly time: Date;
    readonly executionId: string;
    readonly correlationId: string;
    readonly causationId: string | null;
    readonly principalId: string;
    readonly messageGroup: string;
    protected readonly data: TData;
    constructor(base: DomainEventBase, ctx: ExecutionContext, data: TData);
    toDataJson(): string;
    getData(): TData;
}
//# sourceMappingURL=domain-event.d.ts.map