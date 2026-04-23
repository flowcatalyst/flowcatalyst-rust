<?php

declare(strict_types=1);

namespace FlowCatalyst\UseCase;

/**
 * CloudEvents-style fact about what happened in the domain.
 *
 * Concrete events extend `BaseDomainEvent` which populates tracing metadata
 * from an `ExecutionContext`. `DomainEventHelpers` provides subject /
 * messageGroup / eventType formatters.
 */
interface DomainEvent
{
    public function eventId(): string;
    public function eventType(): string;
    public function specVersion(): string;
    public function source(): string;
    public function subject(): string;
    public function time(): \DateTimeImmutable;
    public function executionId(): string;
    public function correlationId(): string;
    public function causationId(): ?string;
    public function principalId(): string;
    public function messageGroup(): string;

    /** JSON-serialised domain-specific payload. */
    public function toDataJson(): string;
}
