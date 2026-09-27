<?php

declare(strict_types=1);

namespace FlowCatalyst\Outbox\DTOs;

use FlowCatalyst\Outbox\QualifiedCode;
use InvalidArgumentException;

/**
 * DTO for creating a dispatch job in the outbox.
 */
class CreateDispatchJobDto
{
    /** The longest descriptor the platform accepts, in characters. */
    public const MAX_DESCRIPTOR_LENGTH = 255;

    /**
     * @param array<string, string> $metadata Additional metadata
     * @param array<string, string> $headers HTTP headers for the webhook
     */
    public function __construct(
        public readonly string $source,
        public readonly string $code,
        public readonly string $targetUrl,
        public readonly string $payload,
        public readonly string $dispatchPoolId,
        public readonly ?string $subject = null,
        public readonly ?string $correlationId = null,
        public readonly ?string $eventId = null,
        public readonly array $metadata = [],
        public readonly array $headers = [],
        public readonly string $payloadContentType = 'application/json',
        public readonly bool $dataOnly = true,
        public readonly ?string $messageGroup = null,
        public readonly ?string $mode = null,
        public readonly ?int $sequence = null,
        public readonly int $timeoutSeconds = 30,
        public readonly int $maxRetries = 5,
        public readonly ?string $retryStrategy = null,
        public readonly ?\DateTimeInterface $scheduledFor = null,
        public readonly ?\DateTimeInterface $expiresAt = null,
        public readonly ?string $idempotencyKey = null,
        public readonly ?string $externalId = null,
        public readonly ?string $connectionId = null,
        public readonly ?string $queue = null,
        public readonly ?string $descriptor = null,
    ) {
        QualifiedCode::assert($this->code, 'Dispatch job code');
        if ($this->descriptor !== null && mb_strlen($this->descriptor, 'UTF-8') > self::MAX_DESCRIPTOR_LENGTH) {
            throw new InvalidArgumentException(
                'Dispatch job descriptor must be at most ' . self::MAX_DESCRIPTOR_LENGTH
                . ' characters, got ' . mb_strlen($this->descriptor, 'UTF-8')
            );
        }
    }

    /**
     * Create a new dispatch job DTO.
     *
     * @param array|string $payload The payload (will be JSON encoded if array)
     */
    public static function create(
        string $source,
        string $code,
        string $targetUrl,
        array|string $payload,
        string $dispatchPoolId,
    ): self {
        return new self(
            source: $source,
            code: $code,
            targetUrl: $targetUrl,
            payload: is_array($payload) ? json_encode($payload) : $payload,
            dispatchPoolId: $dispatchPoolId,
        );
    }

    /**
     * Add a correlation ID.
     */
    public function withCorrelationId(string $correlationId): self
    {
        return new self(
            source: $this->source,
            code: $this->code,
            targetUrl: $this->targetUrl,
            payload: $this->payload,
            dispatchPoolId: $this->dispatchPoolId,
            subject: $this->subject,
            correlationId: $correlationId,
            eventId: $this->eventId,
            metadata: $this->metadata,
            headers: $this->headers,
            payloadContentType: $this->payloadContentType,
            dataOnly: $this->dataOnly,
            messageGroup: $this->messageGroup,
            mode: $this->mode,
            sequence: $this->sequence,
            timeoutSeconds: $this->timeoutSeconds,
            maxRetries: $this->maxRetries,
            retryStrategy: $this->retryStrategy,
            scheduledFor: $this->scheduledFor,
            expiresAt: $this->expiresAt,
            idempotencyKey: $this->idempotencyKey,
            externalId: $this->externalId,
            connectionId: $this->connectionId,
            queue: $this->queue,
            descriptor: $this->descriptor,
        );
    }

    /**
     * Add a subject.
     */
    public function withSubject(string $subject): self
    {
        return new self(
            source: $this->source,
            code: $this->code,
            targetUrl: $this->targetUrl,
            payload: $this->payload,
            dispatchPoolId: $this->dispatchPoolId,
            subject: $subject,
            correlationId: $this->correlationId,
            eventId: $this->eventId,
            metadata: $this->metadata,
            headers: $this->headers,
            payloadContentType: $this->payloadContentType,
            dataOnly: $this->dataOnly,
            messageGroup: $this->messageGroup,
            mode: $this->mode,
            sequence: $this->sequence,
            timeoutSeconds: $this->timeoutSeconds,
            maxRetries: $this->maxRetries,
            retryStrategy: $this->retryStrategy,
            scheduledFor: $this->scheduledFor,
            expiresAt: $this->expiresAt,
            idempotencyKey: $this->idempotencyKey,
            externalId: $this->externalId,
            connectionId: $this->connectionId,
            queue: $this->queue,
            descriptor: $this->descriptor,
        );
    }

    /**
     * Add HTTP headers for the webhook.
     */
    public function withHeaders(array $headers): self
    {
        return new self(
            source: $this->source,
            code: $this->code,
            targetUrl: $this->targetUrl,
            payload: $this->payload,
            dispatchPoolId: $this->dispatchPoolId,
            subject: $this->subject,
            correlationId: $this->correlationId,
            eventId: $this->eventId,
            metadata: $this->metadata,
            headers: array_merge($this->headers, $headers),
            payloadContentType: $this->payloadContentType,
            dataOnly: $this->dataOnly,
            messageGroup: $this->messageGroup,
            mode: $this->mode,
            sequence: $this->sequence,
            timeoutSeconds: $this->timeoutSeconds,
            maxRetries: $this->maxRetries,
            retryStrategy: $this->retryStrategy,
            scheduledFor: $this->scheduledFor,
            expiresAt: $this->expiresAt,
            idempotencyKey: $this->idempotencyKey,
            externalId: $this->externalId,
            connectionId: $this->connectionId,
            queue: $this->queue,
            descriptor: $this->descriptor,
        );
    }

    /**
     * Add metadata.
     */
    public function withMetadata(array $metadata): self
    {
        return new self(
            source: $this->source,
            code: $this->code,
            targetUrl: $this->targetUrl,
            payload: $this->payload,
            dispatchPoolId: $this->dispatchPoolId,
            subject: $this->subject,
            correlationId: $this->correlationId,
            eventId: $this->eventId,
            metadata: array_merge($this->metadata, $metadata),
            headers: $this->headers,
            payloadContentType: $this->payloadContentType,
            dataOnly: $this->dataOnly,
            messageGroup: $this->messageGroup,
            mode: $this->mode,
            sequence: $this->sequence,
            timeoutSeconds: $this->timeoutSeconds,
            maxRetries: $this->maxRetries,
            retryStrategy: $this->retryStrategy,
            scheduledFor: $this->scheduledFor,
            expiresAt: $this->expiresAt,
            idempotencyKey: $this->idempotencyKey,
            externalId: $this->externalId,
            connectionId: $this->connectionId,
            queue: $this->queue,
            descriptor: $this->descriptor,
        );
    }

    /**
     * Set the message group for ordered dispatch.
     */
    public function withMessageGroup(string $messageGroup): self
    {
        return new self(
            source: $this->source,
            code: $this->code,
            targetUrl: $this->targetUrl,
            payload: $this->payload,
            dispatchPoolId: $this->dispatchPoolId,
            subject: $this->subject,
            correlationId: $this->correlationId,
            eventId: $this->eventId,
            metadata: $this->metadata,
            headers: $this->headers,
            payloadContentType: $this->payloadContentType,
            dataOnly: $this->dataOnly,
            messageGroup: $messageGroup,
            mode: $this->mode,
            sequence: $this->sequence,
            timeoutSeconds: $this->timeoutSeconds,
            maxRetries: $this->maxRetries,
            retryStrategy: $this->retryStrategy,
            scheduledFor: $this->scheduledFor,
            expiresAt: $this->expiresAt,
            idempotencyKey: $this->idempotencyKey,
            externalId: $this->externalId,
            connectionId: $this->connectionId,
            queue: $this->queue,
            descriptor: $this->descriptor,
        );
    }

    /**
     * Set the dispatch mode: IMMEDIATE, NEXT_ON_ERROR or BLOCK_ON_ERROR.
     * Controls ordering within the message group; unset defaults to NEXT_ON_ERROR (in-sequence, moving on past a failure).
     */
    public function withMode(string $mode): self
    {
        return new self(
            source: $this->source,
            code: $this->code,
            targetUrl: $this->targetUrl,
            payload: $this->payload,
            dispatchPoolId: $this->dispatchPoolId,
            subject: $this->subject,
            correlationId: $this->correlationId,
            eventId: $this->eventId,
            metadata: $this->metadata,
            headers: $this->headers,
            payloadContentType: $this->payloadContentType,
            dataOnly: $this->dataOnly,
            messageGroup: $this->messageGroup,
            mode: $mode,
            sequence: $this->sequence,
            timeoutSeconds: $this->timeoutSeconds,
            maxRetries: $this->maxRetries,
            retryStrategy: $this->retryStrategy,
            scheduledFor: $this->scheduledFor,
            expiresAt: $this->expiresAt,
            idempotencyKey: $this->idempotencyKey,
            externalId: $this->externalId,
            connectionId: $this->connectionId,
            queue: $this->queue,
            descriptor: $this->descriptor,
        );
    }

    /**
     * Schedule the job for later execution.
     */
    public function scheduledFor(\DateTimeInterface $scheduledFor): self
    {
        return new self(
            source: $this->source,
            code: $this->code,
            targetUrl: $this->targetUrl,
            payload: $this->payload,
            dispatchPoolId: $this->dispatchPoolId,
            subject: $this->subject,
            correlationId: $this->correlationId,
            eventId: $this->eventId,
            metadata: $this->metadata,
            headers: $this->headers,
            payloadContentType: $this->payloadContentType,
            dataOnly: $this->dataOnly,
            messageGroup: $this->messageGroup,
            mode: $this->mode,
            sequence: $this->sequence,
            timeoutSeconds: $this->timeoutSeconds,
            maxRetries: $this->maxRetries,
            retryStrategy: $this->retryStrategy,
            scheduledFor: $scheduledFor,
            expiresAt: $this->expiresAt,
            idempotencyKey: $this->idempotencyKey,
            externalId: $this->externalId,
            connectionId: $this->connectionId,
            queue: $this->queue,
            descriptor: $this->descriptor,
        );
    }

    /**
     * Set an expiration time.
     */
    public function expiresAt(\DateTimeInterface $expiresAt): self
    {
        return new self(
            source: $this->source,
            code: $this->code,
            targetUrl: $this->targetUrl,
            payload: $this->payload,
            dispatchPoolId: $this->dispatchPoolId,
            subject: $this->subject,
            correlationId: $this->correlationId,
            eventId: $this->eventId,
            metadata: $this->metadata,
            headers: $this->headers,
            payloadContentType: $this->payloadContentType,
            dataOnly: $this->dataOnly,
            messageGroup: $this->messageGroup,
            mode: $this->mode,
            sequence: $this->sequence,
            timeoutSeconds: $this->timeoutSeconds,
            maxRetries: $this->maxRetries,
            retryStrategy: $this->retryStrategy,
            scheduledFor: $this->scheduledFor,
            expiresAt: $expiresAt,
            idempotencyKey: $this->idempotencyKey,
            externalId: $this->externalId,
            connectionId: $this->connectionId,
            queue: $this->queue,
            descriptor: $this->descriptor,
        );
    }

    /**
     * Set an idempotency key.
     */
    public function withIdempotencyKey(string $idempotencyKey): self
    {
        return new self(
            source: $this->source,
            code: $this->code,
            targetUrl: $this->targetUrl,
            payload: $this->payload,
            dispatchPoolId: $this->dispatchPoolId,
            subject: $this->subject,
            correlationId: $this->correlationId,
            eventId: $this->eventId,
            metadata: $this->metadata,
            headers: $this->headers,
            payloadContentType: $this->payloadContentType,
            dataOnly: $this->dataOnly,
            messageGroup: $this->messageGroup,
            mode: $this->mode,
            sequence: $this->sequence,
            timeoutSeconds: $this->timeoutSeconds,
            maxRetries: $this->maxRetries,
            retryStrategy: $this->retryStrategy,
            scheduledFor: $this->scheduledFor,
            expiresAt: $this->expiresAt,
            idempotencyKey: $idempotencyKey,
            externalId: $this->externalId,
            connectionId: $this->connectionId,
            queue: $this->queue,
            descriptor: $this->descriptor,
        );
    }

    /**
     * Set the connection ID.
     */
    public function withConnectionId(string $connectionId): self
    {
        return new self(
            source: $this->source,
            code: $this->code,
            targetUrl: $this->targetUrl,
            payload: $this->payload,
            dispatchPoolId: $this->dispatchPoolId,
            subject: $this->subject,
            correlationId: $this->correlationId,
            eventId: $this->eventId,
            metadata: $this->metadata,
            headers: $this->headers,
            payloadContentType: $this->payloadContentType,
            dataOnly: $this->dataOnly,
            messageGroup: $this->messageGroup,
            mode: $this->mode,
            sequence: $this->sequence,
            timeoutSeconds: $this->timeoutSeconds,
            maxRetries: $this->maxRetries,
            retryStrategy: $this->retryStrategy,
            scheduledFor: $this->scheduledFor,
            expiresAt: $this->expiresAt,
            idempotencyKey: $this->idempotencyKey,
            externalId: $this->externalId,
            connectionId: $connectionId,
            queue: $this->queue,
            descriptor: $this->descriptor,
        );
    }

    /**
     * Set the job's own dispatch priority: DEFAULT or HIGH_PRIORITY, matched
     * ignoring case. Unset stays absent — never silently defaulted — so "not
     * asked for" stays distinguishable from an explicit DEFAULT. Wins over
     * the target subscription's own priority at publish time when set.
     *
     * Not validated here: the platform rejects an invalid value, and
     * duplicating that check client-side would just be another place to drift.
     */
    public function withQueue(string $queue): self
    {
        return new self(
            source: $this->source,
            code: $this->code,
            targetUrl: $this->targetUrl,
            payload: $this->payload,
            dispatchPoolId: $this->dispatchPoolId,
            subject: $this->subject,
            correlationId: $this->correlationId,
            eventId: $this->eventId,
            metadata: $this->metadata,
            headers: $this->headers,
            payloadContentType: $this->payloadContentType,
            dataOnly: $this->dataOnly,
            messageGroup: $this->messageGroup,
            mode: $this->mode,
            sequence: $this->sequence,
            timeoutSeconds: $this->timeoutSeconds,
            maxRetries: $this->maxRetries,
            retryStrategy: $this->retryStrategy,
            scheduledFor: $this->scheduledFor,
            expiresAt: $this->expiresAt,
            idempotencyKey: $this->idempotencyKey,
            externalId: $this->externalId,
            connectionId: $this->connectionId,
            queue: $queue,
            descriptor: $this->descriptor,
        );
    }

    /**
     * Describe what the job is, in words (e.g. "Notify Value of user logins"),
     * shown in the platform's dispatch-jobs grid. At most
     * MAX_DESCRIPTOR_LENGTH (255) characters; unset stays absent.
     *
     * @throws InvalidArgumentException when longer than 255 characters (the
     *   platform would answer 400 VALIDATION)
     */
    public function withDescriptor(string $descriptor): self
    {
        return new self(
            source: $this->source,
            code: $this->code,
            targetUrl: $this->targetUrl,
            payload: $this->payload,
            dispatchPoolId: $this->dispatchPoolId,
            subject: $this->subject,
            correlationId: $this->correlationId,
            eventId: $this->eventId,
            metadata: $this->metadata,
            headers: $this->headers,
            payloadContentType: $this->payloadContentType,
            dataOnly: $this->dataOnly,
            messageGroup: $this->messageGroup,
            mode: $this->mode,
            sequence: $this->sequence,
            timeoutSeconds: $this->timeoutSeconds,
            maxRetries: $this->maxRetries,
            retryStrategy: $this->retryStrategy,
            scheduledFor: $this->scheduledFor,
            expiresAt: $this->expiresAt,
            idempotencyKey: $this->idempotencyKey,
            externalId: $this->externalId,
            connectionId: $this->connectionId,
            queue: $this->queue,
            descriptor: $descriptor,
        );
    }

    /**
     * Build the dispatch job payload for the outbox.
     */
    public function toPayload(): array
    {
        return array_filter([
            'source' => $this->source,
            'code' => $this->code,
            'targetUrl' => $this->targetUrl,
            'payload' => $this->payload,
            'payloadContentType' => $this->payloadContentType,
            'dispatchPoolId' => $this->dispatchPoolId,
            'subject' => $this->subject,
            'correlationId' => $this->correlationId,
            'eventId' => $this->eventId,
            'metadata' => !empty($this->metadata) ? $this->metadata : null,
            'headers' => !empty($this->headers) ? $this->headers : null,
            'dataOnly' => $this->dataOnly,
            'messageGroup' => $this->messageGroup,
            'mode' => $this->mode,
            'sequence' => $this->sequence,
            'timeoutSeconds' => $this->timeoutSeconds,
            'maxRetries' => $this->maxRetries,
            'retryStrategy' => $this->retryStrategy,
            'scheduledFor' => $this->scheduledFor?->format('c'),
            'expiresAt' => $this->expiresAt?->format('c'),
            'idempotencyKey' => $this->idempotencyKey,
            'externalId' => $this->externalId,
            'connectionId' => $this->connectionId,
            'queue' => $this->queue,
            'descriptor' => $this->descriptor,
        ], fn($v) => $v !== null);
    }
}
