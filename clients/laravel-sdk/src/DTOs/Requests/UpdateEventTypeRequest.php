<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

/**
 * Payload for PUT /api/event-types/{id}. Only provided fields are updated.
 */
final class UpdateEventTypeRequest
{
    public function __construct(
        public readonly ?string $name = null,
        public readonly ?string $description = null,
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
        return $payload;
    }
}
