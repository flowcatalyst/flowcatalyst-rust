/**
 * ExecutionContext — tracing + principal data for a use case execution.
 *
 * Carries correlation/causation/execution IDs + principal so that emitted
 * domain events share consistent metadata with the HTTP request that
 * triggered them.
 */
import type { DomainEvent } from "./domain-event.js";
export interface ExecutionContext {
    readonly executionId: string;
    readonly correlationId: string;
    readonly causationId: string | null;
    readonly principalId: string;
    readonly initiatedAt: Date;
}
export declare const ExecutionContext: {
    /**
     * Create a fresh context for a new request. Correlation starts as the
     * execution ID (can be overridden with `withCorrelation`).
     */
    create(principalId: string): ExecutionContext;
    /** Use when an inbound request already carries a correlation ID. */
    withCorrelation(principalId: string, correlationId: string): ExecutionContext;
    /**
     * Derive a context from a parent event — preserves correlationId,
     * sets causationId to the parent event's ID.
     */
    fromParentEvent(parent: DomainEvent, principalId: string): ExecutionContext;
};
//# sourceMappingURL=execution-context.d.ts.map