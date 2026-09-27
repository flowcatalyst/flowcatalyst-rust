<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

/**
 * Payload for PUT /api/event-types/{id}. Only provided fields are updated,
 * except `name`, which the platform requires on every update:
 * EventTypes::update() fills it from the current event type when it is null.
 */
final class UpdateEventTypeRequest
{
    public function __construct(
        public readonly ?string $name = null,
        public readonly ?string $description = null,
        public readonly ?bool $clientScoped = null,
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
        if ($this->clientScoped !== null) {
            $payload['clientScoped'] = $this->clientScoped;
        }
        return $payload;
    }
}
