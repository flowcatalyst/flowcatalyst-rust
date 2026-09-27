<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Responses;

use FlowCatalyst\DTOs\ServiceAccountCredentials;

/**
 * Wraps `POST /api/applications/{id}/provision-service-account`:
 * `{message, serviceAccount: {principalId, name, oauthClient: {id, clientId, clientSecret}}}`.
 *
 * The client secret is only returned once. Store it before discarding this
 * object.
 */
final class ProvisionServiceAccountResult
{
    public function __construct(
        public readonly string $message,
        public readonly ServiceAccountCredentials $serviceAccount,
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        /** @var array<string, mixed> $serviceAccount */
        $serviceAccount = is_array($data['serviceAccount'] ?? null) ? $data['serviceAccount'] : [];

        return new self(
            message: (string) ($data['message'] ?? ''),
            serviceAccount: ServiceAccountCredentials::fromArray($serviceAccount),
        );
    }
}
