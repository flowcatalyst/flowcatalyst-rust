<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

use FlowCatalyst\DTOs\EventTypeBinding;
use FlowCatalyst\Enums\DispatchMode;

/**
 * Payload for POST /api/subscriptions.
 *
 * Omit `clientId` for anchor-level subscriptions. `connectionId` ties the
 * delivery to a service account's auth config; omit for plain webhook
 * delivery.
 */
final class CreateSubscriptionRequest
{
    /**
     * @param EventTypeBinding[] $eventTypes
     */
    public function __construct(
        public readonly string $code,
        public readonly string $name,
        public readonly string $endpoint,
        public readonly array $eventTypes,
        public readonly ?string $description = null,
        public readonly ?string $clientId = null,
        public readonly ?string $connectionId = null,
        public readonly ?string $dispatchPoolId = null,
        public readonly ?string $serviceAccountId = null,
        public readonly DispatchMode|string|null $mode = null,
        public readonly ?int $timeoutSeconds = null,
        public readonly ?int $maxRetries = null,
        public readonly bool $dataOnly = false,
    ) {}

    /**
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        $payload = [
            'code' => $this->code,
            'name' => $this->name,
            'endpoint' => $this->endpoint,
            'eventTypes' => array_map(
                fn(EventTypeBinding $b) => $b->toArray(),
                $this->eventTypes,
            ),
            'dataOnly' => $this->dataOnly,
        ];
        if ($this->description !== null) {
            $payload['description'] = $this->description;
        }
        if ($this->clientId !== null) {
            $payload['clientId'] = $this->clientId;
        }
        if ($this->connectionId !== null) {
            $payload['connectionId'] = $this->connectionId;
        }
        if ($this->dispatchPoolId !== null) {
            $payload['dispatchPoolId'] = $this->dispatchPoolId;
        }
        if ($this->serviceAccountId !== null) {
            $payload['serviceAccountId'] = $this->serviceAccountId;
        }
        if ($this->mode !== null) {
            $payload['mode'] = $this->mode instanceof DispatchMode
                ? $this->mode->value
                : $this->mode;
        }
        if ($this->timeoutSeconds !== null) {
            $payload['timeoutSeconds'] = $this->timeoutSeconds;
        }
        if ($this->maxRetries !== null) {
            $payload['maxRetries'] = $this->maxRetries;
        }
        return $payload;
    }
}
