<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

use FlowCatalyst\Enums\ConnectionStatus;

/**
 * Payload for PUT /api/connections/{id}. Only provided fields are updated.
 */
final class UpdateConnectionRequest
{
    public function __construct(
        public readonly ?string $name = null,
        public readonly ?string $description = null,
        public readonly ?string $externalId = null,
        public readonly ConnectionStatus|string|null $status = null,
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
        if ($this->externalId !== null) {
            $payload['externalId'] = $this->externalId;
        }
        if ($this->status !== null) {
            $payload['status'] = $this->status instanceof ConnectionStatus
                ? $this->status->value
                : $this->status;
        }
        return $payload;
    }
}
