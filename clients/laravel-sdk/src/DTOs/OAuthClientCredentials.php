<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs;

/**
 * The OAuth client nested in a provisioning response. `clientSecret` is
 * plaintext and only returned once, at creation, for confidential clients.
 * It is null when the platform did not return one.
 */
final class OAuthClientCredentials
{
    public function __construct(
        public readonly string $id,
        public readonly string $clientId,
        public readonly ?string $clientSecret = null,
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        $secret = $data['clientSecret'] ?? null;

        return new self(
            id: (string) ($data['id'] ?? ''),
            clientId: (string) ($data['clientId'] ?? ''),
            clientSecret: is_string($secret) && $secret !== '' ? $secret : null,
        );
    }

    /**
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        return [
            'id' => $this->id,
            'clientId' => $this->clientId,
            'clientSecret' => $this->clientSecret,
        ];
    }
}
