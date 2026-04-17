<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

/**
 * Payload for PUT /api/applications/{id}. All fields optional.
 */
final class UpdateApplicationRequest
{
    public function __construct(
        public readonly ?string $name = null,
        public readonly ?string $description = null,
        public readonly ?string $defaultBaseUrl = null,
        public readonly ?string $iconUrl = null,
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
        if ($this->defaultBaseUrl !== null) {
            $payload['defaultBaseUrl'] = $this->defaultBaseUrl;
        }
        if ($this->iconUrl !== null) {
            $payload['iconUrl'] = $this->iconUrl;
        }
        return $payload;
    }
}
