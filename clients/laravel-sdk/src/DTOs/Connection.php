<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs;

use FlowCatalyst\Enums\ConnectionStatus;

final class Connection
{
    public function __construct(
        public readonly string $id,
        public readonly string $code,
        public readonly string $name,
        public readonly ConnectionStatus $status,
        public readonly string $serviceAccountId,
        public readonly ?string $description = null,
        public readonly ?string $externalId = null,
        public readonly ?string $clientId = null,
        public readonly ?string $clientIdentifier = null,
        public readonly string $createdAt = '',
        public readonly string $updatedAt = '',
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        return new self(
            id: (string) $data['id'],
            code: (string) $data['code'],
            name: (string) $data['name'],
            status: ConnectionStatus::tryFrom((string) ($data['status'] ?? 'ACTIVE'))
                ?? ConnectionStatus::ACTIVE,
            serviceAccountId: (string) $data['serviceAccountId'],
            description: isset($data['description']) ? (string) $data['description'] : null,
            externalId: isset($data['externalId']) ? (string) $data['externalId'] : null,
            clientId: isset($data['clientId']) ? (string) $data['clientId'] : null,
            clientIdentifier: isset($data['clientIdentifier']) ? (string) $data['clientIdentifier'] : null,
            createdAt: (string) ($data['createdAt'] ?? ''),
            updatedAt: (string) ($data['updatedAt'] ?? ''),
        );
    }

    /**
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        return [
            'id' => $this->id,
            'code' => $this->code,
            'name' => $this->name,
            'status' => $this->status->value,
            'serviceAccountId' => $this->serviceAccountId,
            'description' => $this->description,
            'externalId' => $this->externalId,
            'clientId' => $this->clientId,
            'clientIdentifier' => $this->clientIdentifier,
            'createdAt' => $this->createdAt,
            'updatedAt' => $this->updatedAt,
        ];
    }
}
