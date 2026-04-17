<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs;

/**
 * A grant that gives a principal access to a client.
 *
 * Matches the platform's ClientAccessGrantResponse returned by
 * GET /api/principals/{id}/client-access (inside `grants`) and
 * POST /api/principals/{id}/client-access.
 */
final class ClientAccessGrant
{
    public function __construct(
        public readonly string $id,
        public readonly string $clientId,
        public readonly string $grantedAt,
        public readonly ?string $expiresAt = null,
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        return new self(
            id: (string) $data['id'],
            clientId: (string) $data['clientId'],
            grantedAt: (string) $data['grantedAt'],
            expiresAt: isset($data['expiresAt']) ? (string) $data['expiresAt'] : null,
        );
    }

    /**
     * @return array<string, string|null>
     */
    public function toArray(): array
    {
        return [
            'id' => $this->id,
            'clientId' => $this->clientId,
            'grantedAt' => $this->grantedAt,
            'expiresAt' => $this->expiresAt,
        ];
    }
}
