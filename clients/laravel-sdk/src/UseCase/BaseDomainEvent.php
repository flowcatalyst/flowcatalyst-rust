<?php

declare(strict_types=1);

namespace FlowCatalyst\UseCase;

/**
 * Base class for concrete domain events. Subclasses declare the base fields
 * (eventType / specVersion / source / subject / messageGroup) and the data
 * payload; this class populates the tracing metadata from the
 * `ExecutionContext`.
 *
 * @template TData of array<string, mixed>
 */
abstract class BaseDomainEvent implements DomainEvent
{
    private readonly string $eventId;
    private readonly \DateTimeImmutable $time;
    private readonly string $executionId;
    private readonly string $correlationId;
    private readonly ?string $causationId;
    private readonly string $principalId;

    /**
     * @param array{
     *   eventType: string,
     *   specVersion: string,
     *   source: string,
     *   subject: string,
     *   messageGroup: string,
     * } $base
     * @param TData $data
     */
    public function __construct(
        private readonly array $base,
        ExecutionContext $ctx,
        protected readonly array $data,
    ) {
        $this->eventId       = DomainEventHelpers::generateId();
        $this->time          = new \DateTimeImmutable();
        $this->executionId   = $ctx->executionId;
        $this->correlationId = $ctx->correlationId;
        $this->causationId   = $ctx->causationId;
        $this->principalId   = $ctx->principalId;
    }

    public function eventId(): string         { return $this->eventId; }
    public function eventType(): string       { return $this->base['eventType']; }
    public function specVersion(): string     { return $this->base['specVersion']; }
    public function source(): string          { return $this->base['source']; }
    public function subject(): string         { return $this->base['subject']; }
    public function time(): \DateTimeImmutable { return $this->time; }
    public function executionId(): string     { return $this->executionId; }
    public function correlationId(): string   { return $this->correlationId; }
    public function causationId(): ?string    { return $this->causationId; }
    public function principalId(): string     { return $this->principalId; }
    public function messageGroup(): string    { return $this->base['messageGroup']; }

    public function toDataJson(): string
    {
        return (string) json_encode($this->data);
    }

    /**
     * @return TData
     */
    public function data(): array
    {
        return $this->data;
    }
}
