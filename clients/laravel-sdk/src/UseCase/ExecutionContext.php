<?php

declare(strict_types=1);

namespace FlowCatalyst\UseCase;

use FlowCatalyst\Support\TsidGenerator;

/**
 * Tracing + principal data carried through a single use case execution.
 *
 * Correlation / causation / execution IDs flow into emitted domain events so
 * the platform sees a consistent chain from the originating HTTP request.
 */
final class ExecutionContext
{
    public function __construct(
        public readonly string $executionId,
        public readonly string $correlationId,
        public readonly ?string $causationId,
        public readonly string $principalId,
        public readonly \DateTimeImmutable $initiatedAt,
    ) {}

    /**
     * Fresh context for a brand-new request. Correlation defaults to
     * the execution ID; override with `withCorrelation` when an inbound
     * request carries one.
     */
    public static function create(string $principalId): self
    {
        $execId = self::newExecId();
        return new self(
            executionId:   $execId,
            correlationId: $execId,
            causationId:   null,
            principalId:   $principalId,
            initiatedAt:   new \DateTimeImmutable(),
        );
    }

    public static function withCorrelation(string $principalId, string $correlationId): self
    {
        return new self(
            executionId:   self::newExecId(),
            correlationId: $correlationId,
            causationId:   null,
            principalId:   $principalId,
            initiatedAt:   new \DateTimeImmutable(),
        );
    }

    /**
     * Derive a context from a parent event — preserves correlationId,
     * sets causationId to the parent event's ID.
     */
    public static function fromParentEvent(DomainEvent $parent, string $principalId): self
    {
        return new self(
            executionId:   self::newExecId(),
            correlationId: $parent->correlationId(),
            causationId:   $parent->eventId(),
            principalId:   $principalId,
            initiatedAt:   new \DateTimeImmutable(),
        );
    }

    private static function newExecId(): string
    {
        return 'exec-' . TsidGenerator::generate();
    }
}
