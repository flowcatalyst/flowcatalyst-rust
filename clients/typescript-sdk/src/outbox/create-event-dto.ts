import { assertQualifiedCode } from "./qualified-code.js";

/**
 * DTO for creating an event in the outbox.
 *
 * Uses an immutable builder pattern - all `with*()` methods return a new instance.
 *
 * @example
 * ```typescript
 * const event = CreateEventDto
 *   .create('user.registered', { userId: '123', email: 'a@b.com' })
 *   .withSource('user-service')
 *   .withCorrelationId('corr-456');
 * ```
 */
export class CreateEventDto {
	readonly type: string;
	readonly data: Record<string, unknown>;
	readonly source: string | null;
	readonly subject: string | null;
	readonly correlationId: string | null;
	readonly causationId: string | null;
	readonly deduplicationId: string | null;
	readonly messageGroup: string | null;
	/** FlowCatalyst client (by code) this event belongs to; the platform resolves it to a client_id at ingest. */
	readonly clientCode: string | null;
	readonly contextData: Array<{ key: string; value: string }>;
	readonly headers: Record<string, string>;

	private constructor(params: {
		type: string;
		data: Record<string, unknown>;
		source?: string | null;
		subject?: string | null;
		correlationId?: string | null;
		causationId?: string | null;
		deduplicationId?: string | null;
		messageGroup?: string | null;
		clientCode?: string | null;
		contextData?: Array<{ key: string; value: string }>;
		headers?: Record<string, string>;
	}) {
		this.type = params.type;
		this.data = params.data;
		this.source = params.source ?? null;
		this.subject = params.subject ?? null;
		this.correlationId = params.correlationId ?? null;
		this.causationId = params.causationId ?? null;
		this.deduplicationId = params.deduplicationId ?? null;
		this.messageGroup = params.messageGroup ?? null;
		this.clientCode = params.clientCode ?? null;
		this.contextData = params.contextData ?? [];
		this.headers = params.headers ?? {};
	}

	static create(type: string, data: Record<string, unknown>): CreateEventDto {
		assertQualifiedCode(type, "Event type");
		return new CreateEventDto({ type, data });
	}

	withSource(source: string): CreateEventDto {
		return new CreateEventDto({ ...this.toParams(), source });
	}

	withSubject(subject: string): CreateEventDto {
		return new CreateEventDto({ ...this.toParams(), subject });
	}

	withCorrelationId(correlationId: string): CreateEventDto {
		return new CreateEventDto({ ...this.toParams(), correlationId });
	}

	withCausationId(causationId: string): CreateEventDto {
		return new CreateEventDto({ ...this.toParams(), causationId });
	}

	withDeduplicationId(deduplicationId: string): CreateEventDto {
		return new CreateEventDto({ ...this.toParams(), deduplicationId });
	}

	withMessageGroup(messageGroup: string): CreateEventDto {
		return new CreateEventDto({ ...this.toParams(), messageGroup });
	}

	/** Set the FlowCatalyst client (by code) this event belongs to. */
	withClientCode(clientCode: string): CreateEventDto {
		return new CreateEventDto({ ...this.toParams(), clientCode });
	}

	withHeaders(headers: Record<string, string>): CreateEventDto {
		return new CreateEventDto({
			...this.toParams(),
			headers: { ...this.headers, ...headers },
		});
	}

	withContextData(
		contextData: Array<{ key: string; value: string }>,
	): CreateEventDto {
		return new CreateEventDto({
			...this.toParams(),
			contextData: [...this.contextData, ...contextData],
		});
	}

	/** Build the event payload for the outbox. Filters out null values. */
	toPayload(): Record<string, unknown> {
		return filterNulls({
			specVersion: "1.0",
			type: this.type,
			source: this.source,
			subject: this.subject,
			data: JSON.stringify(this.data),
			correlationId: this.correlationId,
			causationId: this.causationId,
			deduplicationId: this.deduplicationId,
			messageGroup: this.messageGroup,
			clientCode: this.clientCode,
			contextData: this.contextData.length > 0 ? this.contextData : null,
		});
	}

	private toParams() {
		return {
			type: this.type,
			data: this.data,
			source: this.source,
			subject: this.subject,
			correlationId: this.correlationId,
			causationId: this.causationId,
			deduplicationId: this.deduplicationId,
			messageGroup: this.messageGroup,
			clientCode: this.clientCode,
			contextData: this.contextData,
			headers: this.headers,
		};
	}
}

function filterNulls(obj: Record<string, unknown>): Record<string, unknown> {
	const result: Record<string, unknown> = {};
	for (const [key, value] of Object.entries(obj)) {
		if (value !== null && value !== undefined) {
			result[key] = value;
		}
	}
	return result;
}
