/**
 * ExecutionContext — tracing + principal data for a use case execution.
 *
 * Carries correlation/causation/execution IDs + principal so that emitted
 * domain events share consistent metadata with the HTTP request that
 * triggered them.
 */

import { generate as generateTsid } from "../outbox/tsid.js";
import type { DomainEvent } from "./domain-event.js";

export interface ExecutionContext {
	readonly executionId: string;
	readonly correlationId: string;
	readonly causationId: string | null;
	readonly principalId: string;
	readonly initiatedAt: Date;
}

function newExecId(): string {
	return `exec-${generateTsid()}`;
}

export const ExecutionContext = {
	/**
	 * Create a fresh context for a new request. Correlation starts as the
	 * execution ID (can be overridden with `withCorrelation`).
	 */
	create(principalId: string): ExecutionContext {
		const execId = newExecId();
		return {
			executionId: execId,
			correlationId: execId,
			causationId: null,
			principalId,
			initiatedAt: new Date(),
		};
	},

	/** Use when an inbound request already carries a correlation ID. */
	withCorrelation(
		principalId: string,
		correlationId: string,
	): ExecutionContext {
		return {
			executionId: newExecId(),
			correlationId,
			causationId: null,
			principalId,
			initiatedAt: new Date(),
		};
	},

	/**
	 * Derive a context from a parent event — preserves correlationId,
	 * sets causationId to the parent event's ID.
	 */
	fromParentEvent(parent: DomainEvent, principalId: string): ExecutionContext {
		return {
			executionId: newExecId(),
			correlationId: parent.correlationId,
			causationId: parent.eventId,
			principalId,
			initiatedAt: new Date(),
		};
	},
};
