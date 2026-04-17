<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

/**
 * Payload for PUT /api/subscriptions/{id}. Only provided fields are updated.
 *
 * Note: event-type bindings, dispatch pool, and status transitions are
 * not editable here. Use the dedicated pause/resume endpoints for status
 * changes, and re-create / sync to change event-type bindings.
 */
final class UpdateSubscriptionRequest
{
    public function __construct(
        public readonly ?string $name = null,
        public readonly ?string $description = null,
        public readonly ?string $endpoint = null,
        public readonly ?string $connectionId = null,
        public readonly ?int $timeoutSeconds = null,
        public readonly ?int $maxRetries = null,
    ) {}

    /**
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        $payload = [];
        if ($this->name !== null) {
            $payload['name'] = $this->name;
        }
        if ($this->description !== null) {
            $payload['description'] = $this->description;
        }
        if ($this->endpoint !== null) {
            $payload['endpoint'] = $this->endpoint;
        }
        if ($this->connectionId !== null) {
            $payload['connectionId'] = $this->connectionId;
        }
        if ($this->timeoutSeconds !== null) {
            $payload['timeoutSeconds'] = $this->timeoutSeconds;
        }
        if ($this->maxRetries !== null) {
            $payload['maxRetries'] = $this->maxRetries;
        }
        return $payload;
    }
}
