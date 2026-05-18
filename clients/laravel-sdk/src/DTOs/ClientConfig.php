<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs;

/**
 * Per-client configuration for an application — the row that says "client X
 * has application Y enabled, with this base-URL override and these config
 * extras." Returned by `GET /api/applications/{id}/clients` and the
 * enable/disable/update-config endpoints.
 */
final class ClientConfig
{
    /**
     * @param array<string, mixed>|null $config
     */
    public function __construct(
        public readonly string $id,
        public readonly string $applicationId,
        public readonly string $clientId,
        public readonly bool $enabled,
        public readonly ?string $clientName = null,
        public readonly ?string $clientIdentifier = null,
        public readonly ?string $baseUrlOverride = null,
        public readonly ?string $effectiveBaseUrl = null,
        public readonly ?array $config = null,
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        /** @var array<string, mixed>|null $config */
        $config = isset($data['config']) && is_array($data['config']) ? $data['config'] : null;
        return new self(
            id: (string) $data['id'],
            applicationId: (string) $data['applicationId'],
            clientId: (string) $data['clientId'],
            enabled: (bool) ($data['enabled'] ?? false),
            clientName: isset($data['clientName']) ? (string) $data['clientName'] : null,
            clientIdentifier: isset($data['clientIdentifier']) ? (string) $data['clientIdentifier'] : null,
            baseUrlOverride: isset($data['baseUrlOverride']) ? (string) $data['baseUrlOverride'] : null,
            effectiveBaseUrl: isset($data['effectiveBaseUrl']) ? (string) $data['effectiveBaseUrl'] : null,
            config: $config,
        );
    }
}
