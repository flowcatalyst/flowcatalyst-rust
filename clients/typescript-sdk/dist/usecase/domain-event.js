/**
 * DomainEvent — CloudEvents-style facts about what happened.
 *
 * Concrete events extend `BaseDomainEvent<TData>` — they supply the
 * type/source/subject/messageGroup base fields plus the data payload;
 * `BaseDomainEvent` populates eventId/correlation/causation/principal from
 * an `ExecutionContext`.
 */
import { generate as generateTsid } from "../outbox/tsid.js";
export const DomainEvent = {
    generateId() {
        return generateTsid();
    },
    subject(domain, aggregate, id) {
        return `${domain}.${aggregate}.${id}`;
    },
    messageGroup(domain, aggregate, id) {
        return `${domain}:${aggregate}:${id}`;
    },
    eventType(app, domain, aggregate, action) {
        return `${app}:${domain}:${aggregate}:${action}`;
    },
    /** "platform.eventtype.123" → "Eventtype" */
    extractAggregateType(subject) {
        if (!subject)
            return "Unknown";
        const parts = subject.split(".");
        if (parts.length < 2)
            return "Unknown";
        const a = parts[1];
        return a.charAt(0).toUpperCase() + a.slice(1).replace(/-/g, "");
    },
    /** "platform.eventtype.123" → "123" */
    extractEntityId(subject) {
        if (!subject)
            return null;
        const parts = subject.split(".");
        return parts.length >= 3 ? parts[2] : null;
    },
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
export class BaseDomainEvent {
    constructor(base, ctx, data) {
        this.eventId = DomainEvent.generateId();
        this.eventType = base.eventType;
        this.specVersion = base.specVersion;
        this.source = base.source;
        this.subject = base.subject;
        this.time = new Date();
        this.executionId = ctx.executionId;
        this.correlationId = ctx.correlationId;
        this.causationId = ctx.causationId;
        this.principalId = ctx.principalId;
        this.messageGroup = base.messageGroup;
        this.data = data;
    }
    toDataJson() {
        return JSON.stringify(this.data);
    }
    getData() {
        return this.data;
    }
}
