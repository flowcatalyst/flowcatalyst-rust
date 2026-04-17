<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs;

use FlowCatalyst\Enums\SpecVersionStatus;

/**
 * A schema version for an event type.
 *
 * `schema` is only populated on single-event-type responses
 * (GET /api/event-types/{id}), not on list responses.
 */
final class SpecVersion
{
    /**
     * @param array<string, mixed>|null $schema The JSON Schema document
     */
    public function __construct(
        public readonly string $version,
        public readonly SpecVersionStatus $status,
        public readonly ?array $schema = null,
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        /** @var array<string, mixed>|null $schema */
        $schema = $data['schema'] ?? null;
        return new self(
            version: (string) $data['version'],
            status: SpecVersionStatus::tryFrom((string) ($data['status'] ?? 'CURRENT'))
                ?? SpecVersionStatus::CURRENT,
            schema: is_array($schema) ? $schema : null,
        );
    }

    /**
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        return [
            'version' => $this->version,
            'status' => $this->status->value,
            'schema' => $this->schema,
        ];
    }

    /**
     * Get the major version number.
     */
    public function getMajorVersion(): int
    {
        $parts = explode('.', $this->version);
        return (int) ($parts[0] ?? 0);
    }

    /**
     * Get the minor version number.
     */
    public function getMinorVersion(): int
    {
        $parts = explode('.', $this->version);
        return (int) ($parts[1] ?? 0);
    }
}
