<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

/**
 * Payload for PUT /api/clients/{id}. Only provided fields are updated.
 */
final class UpdateClientRequest
{
    public function __construct(
        public readonly ?string $name = null,
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
        return $payload;
    }
}
