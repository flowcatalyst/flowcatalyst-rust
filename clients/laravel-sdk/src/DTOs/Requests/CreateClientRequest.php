<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

/**
 * Payload for POST /api/clients.
 */
final class CreateClientRequest
{
    public function __construct(
        public readonly string $identifier,
        public readonly string $name,
    ) {}

    /**
     * @return array<string, string>
     */
    public function toArray(): array
    {
        return [
            'identifier' => $this->identifier,
            'name' => $this->name,
        ];
    }
}
