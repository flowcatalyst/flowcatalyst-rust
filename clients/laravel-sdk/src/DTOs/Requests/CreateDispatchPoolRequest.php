<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

/**
 * Payload for POST /api/dispatch-pools.
 *
 * Omit `clientId` for anchor-level pools. `rateLimit` is messages per
 * minute; `concurrency` is the maximum parallel dispatches.
 */
final class CreateDispatchPoolRequest
{
    public function __construct(
        public readonly string $code,
        public readonly string $name,
        public readonly ?string $description = null,
        public readonly ?string $clientId = null,
        public readonly ?int $rateLimit = null,
        public readonly ?int $concurrency = null,
    ) {}

    /**
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        $payload = [
            'code' => $this->code,
            'name' => $this->name,
        ];
        if ($this->description !== null) {
            $payload['description'] = $this->description;
        }
        if ($this->clientId !== null) {
            $payload['clientId'] = $this->clientId;
        }
        if ($this->rateLimit !== null) {
            $payload['rateLimit'] = $this->rateLimit;
        }
        if ($this->concurrency !== null) {
            $payload['concurrency'] = $this->concurrency;
        }
        return $payload;
    }
}
