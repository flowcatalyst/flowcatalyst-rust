<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs;

/**
 * The service account block of a provision-service-account response: the
 * service principal and its OAuth client, whose one-time secret is in
 * `$oauthClient->clientSecret`.
 */
final class ServiceAccountCredentials
{
    public function __construct(
        public readonly string $principalId,
        public readonly string $name,
        public readonly OAuthClientCredentials $oauthClient,
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        /** @var array<string, mixed> $oauthClient */
        $oauthClient = is_array($data['oauthClient'] ?? null) ? $data['oauthClient'] : [];

        return new self(
            principalId: (string) ($data['principalId'] ?? ''),
            name: (string) ($data['name'] ?? ''),
            oauthClient: OAuthClientCredentials::fromArray($oauthClient),
        );
    }

    /**
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        return [
            'principalId' => $this->principalId,
            'name' => $this->name,
            'oauthClient' => $this->oauthClient->toArray(),
        ];
    }
}
