<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

/**
 * One entry in the POST /api/applications/{appCode}/connections/sync
 * payload. A connection carries nothing environment-specific — the platform
 * assigns the application's own provisioned service account to it.
 */
final class SyncConnectionEntry
{
    public function __construct(
        public readonly string $code,
        public readonly string $name,
        public readonly ?string $description = null,
        public readonly ?string $externalId = null,
    ) {}

    /**
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        $payload = [
            'code' => $this->code,
            'name' => $this->name,
        ];
        if ($this->description !== null) {
            $payload['description'] = $this->description;
        }
        if ($this->externalId !== null) {
            $payload['externalId'] = $this->externalId;
        }
        return $payload;
    }
}
