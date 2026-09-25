<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

/**
 * Payload for POST /api/event-types/{id}/versions.
 *
 * `version` is required by the platform — a body without it is rejected with a
 * 400 ("must follow format ...") — and the platform does NOT auto-increment:
 * it stores the version string verbatim and rejects a duplicate
 * (event-type, version) pair. Callers that don't care default to
 * {@see self::DEFAULT_VERSION}. The mime-type is hardcoded server-side to
 * `application/schema+json`.
 */
final class AddSchemaVersionRequest
{
    /** Default schema version when the caller doesn't specify one. */
    public const DEFAULT_VERSION = '1.0';

    /**
     * @param array<string, mixed> $schema  JSON Schema document
     * @param string               $version Schema version (e.g. "1.0"); sent as-is
     */
    public function __construct(
        public readonly array $schema,
        public readonly string $version = self::DEFAULT_VERSION,
    ) {}

    /**
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        return [
            'version' => $this->version !== '' ? $this->version : self::DEFAULT_VERSION,
            'schema' => $this->schema,
        ];
    }
}
