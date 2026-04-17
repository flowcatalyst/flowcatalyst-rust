<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

/**
 * Payload for POST /api/event-types/{id}/versions.
 *
 * The platform auto-increments the version number and hardcodes the
 * mime-type to `application/schema+json`. Only the JSON Schema itself
 * is configurable.
 */
final class AddSchemaVersionRequest
{
    /**
     * @param array<string, mixed> $schema JSON Schema document
     */
    public function __construct(
        public readonly array $schema,
    ) {}

    /**
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        return [
            'schema' => $this->schema,
        ];
    }
}
