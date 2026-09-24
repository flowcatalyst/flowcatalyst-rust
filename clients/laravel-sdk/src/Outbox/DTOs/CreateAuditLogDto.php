<?php

declare(strict_types=1);

namespace FlowCatalyst\Outbox\DTOs;

use FlowCatalyst\Outbox\AuditRedaction;

/**
 * DTO for creating an audit log entry in the outbox.
 *
 * `operationData` is redacted (see {@see AuditRedaction}) before it is
 * serialised into the outbox payload, so passwords, tokens and other
 * secret-shaped fields never reach `aud_logs`. Pass `$maskedFields` to
 * `withOperationData()` for fields the name rule alone would not catch.
 */
class CreateAuditLogDto
{
    /**
     * @param array<string, mixed>|null $operationData Operation payload data
     * @param array<string, string> $metadata Additional metadata
     * @param array<string, string> $headers Optional headers
     * @param list<string> $maskedFields Top-level field names to mask in
     *   $operationData in addition to the name rule
     */
    public function __construct(
        public readonly string $entityType,
        public readonly string $entityId,
        public readonly string $operation,
        public readonly ?array $operationData = null,
        public readonly ?string $principalId = null,
        public readonly ?\DateTimeInterface $performedAt = null,
        public readonly ?string $source = null,
        public readonly ?string $correlationId = null,
        public readonly array $metadata = [],
        public readonly array $headers = [],
        public readonly array $maskedFields = [],
    ) {}

    /**
     * Create a new audit log DTO.
     */
    public static function create(
        string $entityType,
        string $entityId,
        string $operation,
    ): self {
        return new self(
            entityType: $entityType,
            entityId: $entityId,
            operation: $operation,
        );
    }

    /**
     * Add operation data.
     *
     * @param list<string> $maskedFields Top-level field names to mask in
     *   addition to the name rule (see {@see AuditRedaction}), e.g. a
     *   config value whose secrecy depends on a sibling field.
     */
    public function withOperationData(array $operationData, array $maskedFields = []): self
    {
        return new self(
            entityType: $this->entityType,
            entityId: $this->entityId,
            operation: $this->operation,
            operationData: $operationData,
            principalId: $this->principalId,
            performedAt: $this->performedAt,
            source: $this->source,
            correlationId: $this->correlationId,
            metadata: $this->metadata,
            headers: $this->headers,
            maskedFields: $maskedFields,
        );
    }

    /**
     * Add a principal ID.
     */
    public function withPrincipalId(string $principalId): self
    {
        return new self(
            entityType: $this->entityType,
            entityId: $this->entityId,
            operation: $this->operation,
            operationData: $this->operationData,
            principalId: $principalId,
            performedAt: $this->performedAt,
            source: $this->source,
            correlationId: $this->correlationId,
            metadata: $this->metadata,
            headers: $this->headers,
            maskedFields: $this->maskedFields,
        );
    }

    /**
     * Set when the operation was performed.
     */
    public function withPerformedAt(\DateTimeInterface $performedAt): self
    {
        return new self(
            entityType: $this->entityType,
            entityId: $this->entityId,
            operation: $this->operation,
            operationData: $this->operationData,
            principalId: $this->principalId,
            performedAt: $performedAt,
            source: $this->source,
            correlationId: $this->correlationId,
            metadata: $this->metadata,
            headers: $this->headers,
            maskedFields: $this->maskedFields,
        );
    }

    /**
     * Add a source.
     */
    public function withSource(string $source): self
    {
        return new self(
            entityType: $this->entityType,
            entityId: $this->entityId,
            operation: $this->operation,
            operationData: $this->operationData,
            principalId: $this->principalId,
            performedAt: $this->performedAt,
            source: $source,
            correlationId: $this->correlationId,
            metadata: $this->metadata,
            headers: $this->headers,
            maskedFields: $this->maskedFields,
        );
    }

    /**
     * Add a correlation ID.
     */
    public function withCorrelationId(string $correlationId): self
    {
        return new self(
            entityType: $this->entityType,
            entityId: $this->entityId,
            operation: $this->operation,
            operationData: $this->operationData,
            principalId: $this->principalId,
            performedAt: $this->performedAt,
            source: $this->source,
            correlationId: $correlationId,
            metadata: $this->metadata,
            headers: $this->headers,
            maskedFields: $this->maskedFields,
        );
    }

    /**
     * Add metadata.
     */
    public function withMetadata(array $metadata): self
    {
        return new self(
            entityType: $this->entityType,
            entityId: $this->entityId,
            operation: $this->operation,
            operationData: $this->operationData,
            principalId: $this->principalId,
            performedAt: $this->performedAt,
            source: $this->source,
            correlationId: $this->correlationId,
            metadata: array_merge($this->metadata, $metadata),
            headers: $this->headers,
            maskedFields: $this->maskedFields,
        );
    }

    /**
     * Add headers.
     */
    public function withHeaders(array $headers): self
    {
        return new self(
            entityType: $this->entityType,
            entityId: $this->entityId,
            operation: $this->operation,
            operationData: $this->operationData,
            principalId: $this->principalId,
            performedAt: $this->performedAt,
            source: $this->source,
            correlationId: $this->correlationId,
            metadata: $this->metadata,
            headers: array_merge($this->headers, $headers),
            maskedFields: $this->maskedFields,
        );
    }

    /**
     * Build the audit log payload for the outbox.
     */
    public function toPayload(): array
    {
        return array_filter([
            'entityType' => $this->entityType,
            'entityId' => $this->entityId,
            'operation' => $this->operation,
            'operationData' => $this->operationData !== null
                ? json_encode(AuditRedaction::redact($this->operationData, $this->maskedFields))
                : null,
            'principalId' => $this->principalId,
            'performedAt' => ($this->performedAt ?? new \DateTimeImmutable())->format('c'),
            'source' => $this->source,
            'correlationId' => $this->correlationId,
            'metadata' => !empty($this->metadata) ? $this->metadata : null,
        ], fn($v) => $v !== null);
    }
}
