<?php

declare(strict_types=1);

namespace FlowCatalyst\UseCase;

/**
 * Operation — one business operation expressed as named phases, run by
 * {@link Runner::run()}. The PHP port of the Go SDK's `usecaseop.Operation`.
 *
 *   validate  → command shape (pure, no I/O); return null to pass
 *   authorize → resource-level access; return null to allow (the "public" case)
 *   execute   → load + invariants → return a Plan (or a UseCaseError)
 *   ───────── the Runner then applies the Plan in one transaction
 *
 * An operation cannot reach the database except by returning a {@link Plan};
 * the Runner is the only thing that applies one, inside a transaction the unit
 * of work owns. So "aggregate written ⇒ event written, atomically" holds by
 * construction.
 *
 * @template TCommand of Command
 * @template TEvent of DomainEvent
 */
interface Operation
{
    /**
     * Check command shape — presence, format, length, patterns; nothing that
     * loads data. Return a validation-kind {@link UseCaseError}, or null to pass.
     *
     * @param TCommand $command
     */
    public function validate(Command $command): ?UseCaseError;

    /**
     * Resource-level access check (ownership, scope, state). Return a
     * {@link UseCaseError} to deny, or null to allow. Returning null is the
     * explicit "intentionally open" decision (coarse RBAC stays at the
     * controller). Mirrors the Go SDK's `usecaseop.Public`.
     *
     * @param TCommand $command
     */
    public function authorize(Command $command, ExecutionContext $context): ?UseCaseError;

    /**
     * Run invariant checks (load aggregates, apply business rules), build the
     * domain event, and return the {@link Plan} describing the change. Returning
     * a Plan is the only way to reach the database. Return a {@link UseCaseError}
     * to fail without committing.
     *
     * @param TCommand $command
     * @return Plan<TEvent>|UseCaseError
     */
    public function execute(Command $command, ExecutionContext $context): Plan|UseCaseError;
}
