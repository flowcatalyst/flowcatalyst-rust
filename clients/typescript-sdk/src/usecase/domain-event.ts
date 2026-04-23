/**
 * DomainEvent — CloudEvents-style facts about what happened.
 *
 * Concrete events extend `BaseDomainEvent<TData>` — they supply the
 * type/source/subject/messageGroup base fields plus the data payload;
 * `BaseDomainEvent` populates eventId/correlation/causation/principal from
 * an `ExecutionContext`.
 */

import { generate as generateTsid } from "../outbox/tsid.js";
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

export const DomainEvent = {
	generateId(): string {
		return generateTsid();
	},

	subject(domain: string, aggregate: string, id: string): string {
		return `${domain}.${aggregate}.${id}`;
	},

	messageGroup(domain: string, aggregate: string, id: string): string {
		return `${domain}:${aggregate}:${id}`;
	},

	eventType(
		app: string,
		domain: string,
		aggregate: string,
		action: string,
	): string {
		return `${app}:${domain}:${aggregate}:${action}`;
	},

	/** "platform.eventtype.123" → "Eventtype" */
	extractAggregateType(subject: string): string {
		if (!subject) return "Unknown";
		const parts = subject.split(".");
		if (parts.length < 2) return "Unknown";
		const a = parts[1]!;
		return a.charAt(0).toUpperCase() + a.slice(1).replace(/-/g, "");
	},

	/** "platform.eventtype.123" → "123" */
	extractEntityId(subject: string): string | null {
		if (!subject) return null;
		const parts = subject.split(".");
		return parts.length >= 3 ? parts[2]! : null;
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
export abstract class BaseDomainEvent<TData extends object>
	implements DomainEvent
{
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

	constructor(base: DomainEventBase, ctx: ExecutionContext, data: TData) {
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

	toDataJson(): string {
		return JSON.stringify(this.data);
	}

	getData(): TData {
		return this.data;
	}
}
