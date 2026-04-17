<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

use FlowCatalyst\DTOs\EventTypeBinding;
use FlowCatalyst\Enums\DispatchMode;

/**
 * One entry in the POST /api/applications/{appCode}/subscriptions/sync
 * payload.
 *
 * `target` is the webhook URL (the platform calls this `endpoint` on the
 * single-subscription create path — names differ between the two).
 * `dispatchPoolCode` is the pool's code (not ID) for declarative sync.
 */
final class SyncSubscriptionEntry
{
    /**
     * @param EventTypeBinding[] $eventTypes
     */
    public function __construct(
        public readonly string $code,
        public readonly string $name,
        public readonly string $target,
        public readonly array $eventTypes,
        public readonly ?string $description = null,
        public readonly ?string $connectionId = null,
        public readonly ?string $dispatchPoolCode = null,
        public readonly DispatchMode|string|null $mode = null,
        public readonly ?int $maxRetries = null,
        public readonly ?int $timeoutSeconds = null,
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
            'target' => $this->target,
            'eventTypes' => array_map(
                fn(EventTypeBinding $b) => $b->toArray(),
                $this->eventTypes,
            ),
            'dataOnly' => $this->dataOnly,
        ];
        if ($this->description !== null) {
            $payload['description'] = $this->description;
        }
        if ($this->connectionId !== null) {
            $payload['connectionId'] = $this->connectionId;
        }
        if ($this->dispatchPoolCode !== null) {
            $payload['dispatchPoolCode'] = $this->dispatchPoolCode;
        }
        if ($this->mode !== null) {
            $payload['mode'] = $this->mode instanceof DispatchMode
                ? $this->mode->value
                : $this->mode;
        }
        if ($this->maxRetries !== null) {
            $payload['maxRetries'] = $this->maxRetries;
        }
        if ($this->timeoutSeconds !== null) {
            $payload['timeoutSeconds'] = $this->timeoutSeconds;
        }
        return $payload;
    }
}
