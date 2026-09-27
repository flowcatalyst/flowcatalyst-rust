<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs;

/**
 * Per-client configuration for an application — the row that says "client X
 * has application Y enabled, with this base-URL override and these config
 * extras." Returned by `GET /api/applications/{id}/clients` and
 * `GET /api/applications/{id}/clients/{clientId}`.
 *
 * The platform sends the config extras as `configJson`; `$config` holds them
 * (older platforms sent `config`). `clientName`, `clientIdentifier` and
 * `effectiveBaseUrl` are only filled by platforms that send them.
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
        public readonly ?string $createdAt = null,
        public readonly ?string $updatedAt = null,
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        /** @var array<string, mixed>|null $config */
        $raw = $data['configJson'] ?? $data['config'] ?? null;
        $config = is_array($raw) ? $raw : null;
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
            createdAt: isset($data['createdAt']) ? (string) $data['createdAt'] : null,
            updatedAt: isset($data['updatedAt']) ? (string) $data['updatedAt'] : null,
        );
    }
}
