<?php

declare(strict_types=1);

namespace FlowCatalyst\Outbox\DTOs;

use FlowCatalyst\Outbox\QualifiedCode;

/**
 * DTO for creating an event in the outbox.
 */
class CreateEventDto
{
    /**
     * @param array<string, mixed> $data Event payload data
     * @param array<array{key: string, value: string}> $contextData Additional context
     * @param array<string, mixed> $headers Optional headers
     */
    public function __construct(
        public readonly string $type,
        public readonly array $data,
        public readonly ?string $source = null,
        public readonly ?string $subject = null,
        public readonly ?string $correlationId = null,
        public readonly ?string $causationId = null,
        public readonly ?string $deduplicationId = null,
        public readonly ?string $messageGroup = null,
        public readonly ?string $clientCode = null,
        public readonly array $contextData = [],
        public readonly array $headers = [],
    ) {
        QualifiedCode::assert($this->type, 'Event type');
    }

    /**
     * Create a new event DTO.
     */
    public static function create(string $type, array $data): self
    {
        return new self(
            type: $type,
            data: $data,
        );
    }

    /**
     * Add a correlation ID.
     */
    public function withCorrelationId(string $correlationId): self
    {
        return new self(
            type: $this->type,
            data: $this->data,
            source: $this->source,
            subject: $this->subject,
            correlationId: $correlationId,
            causationId: $this->causationId,
            deduplicationId: $this->deduplicationId,
            messageGroup: $this->messageGroup,
            clientCode: $this->clientCode,
            contextData: $this->contextData,
            headers: $this->headers,
        );
    }

    /**
     * Add a causation ID.
     */
    public function withCausationId(string $causationId): self
    {
        return new self(
            type: $this->type,
            data: $this->data,
            source: $this->source,
            subject: $this->subject,
            correlationId: $this->correlationId,
            causationId: $causationId,
            deduplicationId: $this->deduplicationId,
            messageGroup: $this->messageGroup,
            clientCode: $this->clientCode,
            contextData: $this->contextData,
            headers: $this->headers,
        );
    }

    /**
     * Add a deduplication ID.
     */
    public function withDeduplicationId(string $deduplicationId): self
    {
        return new self(
            type: $this->type,
            data: $this->data,
            source: $this->source,
            subject: $this->subject,
            correlationId: $this->correlationId,
            causationId: $this->causationId,
            deduplicationId: $deduplicationId,
            messageGroup: $this->messageGroup,
            clientCode: $this->clientCode,
            contextData: $this->contextData,
            headers: $this->headers,
        );
    }

    /**
     * Add a source.
     */
    public function withSource(string $source): self
    {
        return new self(
            type: $this->type,
            data: $this->data,
            source: $source,
            subject: $this->subject,
            correlationId: $this->correlationId,
            causationId: $this->causationId,
            deduplicationId: $this->deduplicationId,
            messageGroup: $this->messageGroup,
            clientCode: $this->clientCode,
            contextData: $this->contextData,
            headers: $this->headers,
        );
    }

    /**
     * Add a subject.
     */
    public function withSubject(string $subject): self
    {
        return new self(
            type: $this->type,
            data: $this->data,
            source: $this->source,
            subject: $subject,
            correlationId: $this->correlationId,
            causationId: $this->causationId,
            deduplicationId: $this->deduplicationId,
            messageGroup: $this->messageGroup,
            clientCode: $this->clientCode,
            contextData: $this->contextData,
            headers: $this->headers,
        );
    }

    /**
     * Add a message group for ordering.
     */
    public function withMessageGroup(string $messageGroup): self
    {
        return new self(
            type: $this->type,
            data: $this->data,
            source: $this->source,
            subject: $this->subject,
            correlationId: $this->correlationId,
            causationId: $this->causationId,
            deduplicationId: $this->deduplicationId,
            messageGroup: $messageGroup,
            clientCode: $this->clientCode,
            contextData: $this->contextData,
            headers: $this->headers,
        );
    }

    /**
     * Set the FlowCatalyst client (by code) this event belongs to. The platform
     * resolves the code to a client id at ingest.
     */
    public function withClientCode(string $clientCode): self
    {
        return new self(
            type: $this->type,
            data: $this->data,
            source: $this->source,
            subject: $this->subject,
            correlationId: $this->correlationId,
            causationId: $this->causationId,
            deduplicationId: $this->deduplicationId,
            messageGroup: $this->messageGroup,
            clientCode: $clientCode,
            contextData: $this->contextData,
            headers: $this->headers,
        );
    }

    /**
     * Add headers.
     */
    public function withHeaders(array $headers): self
    {
        return new self(
            type: $this->type,
            data: $this->data,
            source: $this->source,
            subject: $this->subject,
            correlationId: $this->correlationId,
            causationId: $this->causationId,
            deduplicationId: $this->deduplicationId,
            messageGroup: $this->messageGroup,
            clientCode: $this->clientCode,
            contextData: $this->contextData,
            headers: array_merge($this->headers, $headers),
        );
    }

    /**
     * Add context data.
     */
    public function withContextData(array $contextData): self
    {
        return new self(
            type: $this->type,
            data: $this->data,
            source: $this->source,
            subject: $this->subject,
            correlationId: $this->correlationId,
            causationId: $this->causationId,
            deduplicationId: $this->deduplicationId,
            messageGroup: $this->messageGroup,
            clientCode: $this->clientCode,
            contextData: array_merge($this->contextData, $contextData),
            headers: $this->headers,
        );
    }

    /**
     * Build the event payload for the outbox.
     */
    public function toPayload(): array
    {
        return array_filter([
            'specVersion' => '1.0',
            'type' => $this->type,
            'source' => $this->source,
            'subject' => $this->subject,
            'data' => json_encode($this->data),
            'correlationId' => $this->correlationId,
            'causationId' => $this->causationId,
            'deduplicationId' => $this->deduplicationId,
            'messageGroup' => $this->messageGroup,
            'clientCode' => $this->clientCode,
            'contextData' => !empty($this->contextData) ? $this->contextData : null,
        ], fn($v) => $v !== null);
    }
}
