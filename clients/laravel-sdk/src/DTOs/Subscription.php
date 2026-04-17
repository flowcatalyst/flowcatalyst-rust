<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs;

use FlowCatalyst\Enums\DispatchMode;
use FlowCatalyst\Enums\SubscriptionStatus;

/**
 * A webhook subscription.
 *
 * Matches the platform's SubscriptionResponse. `endpoint` is the
 * webhook URL the platform POSTs to; `connectionId` (optional) ties
 * the delivery to a service account's auth config.
 */
final class Subscription
{
    /**
     * @param EventTypeBinding[] $eventTypes
     * @param array<int, array{key: string, value: string}> $customConfig
     */
    public function __construct(
        public readonly string $id,
        public readonly string $code,
        public readonly string $name,
        public readonly string $endpoint,
        public readonly SubscriptionStatus $status,
        public readonly DispatchMode $mode,
        public readonly array $eventTypes,
        public readonly array $customConfig,
        public readonly bool $dataOnly,
        public readonly bool $clientScoped,
        public readonly int $maxAgeSeconds,
        public readonly int $delaySeconds,
        public readonly int $sequence,
        public readonly int $timeoutSeconds,
        public readonly int $maxRetries,
        public readonly string $createdAt,
        public readonly string $updatedAt,
        public readonly ?string $description = null,
        public readonly ?string $clientId = null,
        public readonly ?string $clientIdentifier = null,
        public readonly ?string $connectionId = null,
        public readonly ?string $queue = null,
        public readonly ?string $dispatchPoolId = null,
        public readonly ?string $dispatchPoolCode = null,
        public readonly ?string $serviceAccountId = null,
        public readonly ?string $applicationCode = null,
        public readonly ?string $source = null,
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        /** @var array<int, array<string, mixed>> $bindingRows */
        $bindingRows = $data['eventTypes'] ?? [];
        /** @var array<int, array{key: string, value: string}> $customConfig */
        $customConfig = $data['customConfig'] ?? [];

        return new self(
            id: (string) $data['id'],
            code: (string) $data['code'],
            name: (string) $data['name'],
            endpoint: (string) ($data['endpoint'] ?? ''),
            status: SubscriptionStatus::tryFrom((string) ($data['status'] ?? 'ACTIVE'))
                ?? SubscriptionStatus::ACTIVE,
            mode: DispatchMode::tryFrom((string) ($data['mode'] ?? 'IMMEDIATE'))
                ?? DispatchMode::IMMEDIATE,
            eventTypes: array_map(
                fn(array $row) => EventTypeBinding::fromArray($row),
                $bindingRows,
            ),
            customConfig: $customConfig,
            dataOnly: (bool) ($data['dataOnly'] ?? true),
            clientScoped: (bool) ($data['clientScoped'] ?? false),
            maxAgeSeconds: (int) ($data['maxAgeSeconds'] ?? 86400),
            delaySeconds: (int) ($data['delaySeconds'] ?? 0),
            sequence: (int) ($data['sequence'] ?? 99),
            timeoutSeconds: (int) ($data['timeoutSeconds'] ?? 30),
            maxRetries: (int) ($data['maxRetries'] ?? 0),
            createdAt: (string) ($data['createdAt'] ?? ''),
            updatedAt: (string) ($data['updatedAt'] ?? ''),
            description: isset($data['description']) ? (string) $data['description'] : null,
            clientId: isset($data['clientId']) ? (string) $data['clientId'] : null,
            clientIdentifier: isset($data['clientIdentifier']) ? (string) $data['clientIdentifier'] : null,
            connectionId: isset($data['connectionId']) ? (string) $data['connectionId'] : null,
            queue: isset($data['queue']) ? (string) $data['queue'] : null,
            dispatchPoolId: isset($data['dispatchPoolId']) ? (string) $data['dispatchPoolId'] : null,
            dispatchPoolCode: isset($data['dispatchPoolCode']) ? (string) $data['dispatchPoolCode'] : null,
            serviceAccountId: isset($data['serviceAccountId']) ? (string) $data['serviceAccountId'] : null,
            applicationCode: isset($data['applicationCode']) ? (string) $data['applicationCode'] : null,
            source: isset($data['source']) ? (string) $data['source'] : null,
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
            'endpoint' => $this->endpoint,
            'status' => $this->status->value,
            'mode' => $this->mode->value,
            'eventTypes' => array_map(fn(EventTypeBinding $b) => $b->toArray(), $this->eventTypes),
            'customConfig' => $this->customConfig,
            'dataOnly' => $this->dataOnly,
            'clientScoped' => $this->clientScoped,
            'maxAgeSeconds' => $this->maxAgeSeconds,
            'delaySeconds' => $this->delaySeconds,
            'sequence' => $this->sequence,
            'timeoutSeconds' => $this->timeoutSeconds,
            'maxRetries' => $this->maxRetries,
            'createdAt' => $this->createdAt,
            'updatedAt' => $this->updatedAt,
            'description' => $this->description,
            'clientId' => $this->clientId,
            'clientIdentifier' => $this->clientIdentifier,
            'connectionId' => $this->connectionId,
            'queue' => $this->queue,
            'dispatchPoolId' => $this->dispatchPoolId,
            'dispatchPoolCode' => $this->dispatchPoolCode,
            'serviceAccountId' => $this->serviceAccountId,
            'applicationCode' => $this->applicationCode,
            'source' => $this->source,
        ];
    }
}
