<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

/**
 * One entry in the POST /api/applications/{appCode}/event-types/sync payload.
 *
 * `code` should be the full code (`{app}:{subdomain}:{aggregate}:{event}`).
 */
final class SyncEventTypeEntry
{
    public function __construct(
        public readonly string $code,
        public readonly string $name,
        public readonly ?string $description = null,
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
        return $payload;
    }
}
