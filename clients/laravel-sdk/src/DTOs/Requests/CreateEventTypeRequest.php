<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

/**
 * Payload for POST /api/event-types.
 *
 * `code` is formatted `{application}:{subdomain}:{aggregate}:{event}`.
 * `schema` (optional) seeds the first schema version as JSON Schema.
 * Omit `clientId` for anchor-level event types.
 */
final class CreateEventTypeRequest
{
    /**
     * @param array<string, mixed>|null $schema JSON Schema document
     */
    public function __construct(
        public readonly string $code,
        public readonly string $name,
        public readonly ?string $description = null,
        public readonly ?array $schema = null,
        public readonly ?string $clientId = null,
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
        if ($this->schema !== null) {
            $payload['schema'] = $this->schema;
        }
        if ($this->clientId !== null) {
            $payload['clientId'] = $this->clientId;
        }
        return $payload;
    }
}
