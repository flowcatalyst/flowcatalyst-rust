<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

/**
 * Payload for PUT /api/principals/{id}.
 *
 * Each field is optional. Only provided fields are sent to the platform;
 * unset fields are left unchanged.
 */
final class UpdatePrincipalRequest
{
    public function __construct(
        public readonly ?string $name = null,
        public readonly ?bool $active = null,
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
        if ($this->active !== null) {
            $payload['active'] = $this->active;
        }
        return $payload;
    }
}
