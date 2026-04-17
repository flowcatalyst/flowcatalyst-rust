<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

/**
 * Payload for POST /api/connections.
 *
 * A connection binds a service account (the authenticator) to an optional
 * client scope. External ID lets you tag the connection with an ID from
 * your own system.
 */
final class CreateConnectionRequest
{
    public function __construct(
        public readonly string $code,
        public readonly string $name,
        public readonly string $serviceAccountId,
        public readonly ?string $description = null,
        public readonly ?string $externalId = null,
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
            'serviceAccountId' => $this->serviceAccountId,
        ];
        if ($this->description !== null) {
            $payload['description'] = $this->description;
        }
        if ($this->externalId !== null) {
            $payload['externalId'] = $this->externalId;
        }
        if ($this->clientId !== null) {
            $payload['clientId'] = $this->clientId;
        }
        return $payload;
    }
}
