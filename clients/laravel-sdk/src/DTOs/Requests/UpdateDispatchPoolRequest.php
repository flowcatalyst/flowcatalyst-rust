<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

/**
 * Payload for PUT /api/dispatch-pools/{id}. Only provided fields are updated.
 *
 * Status transitions are not done via update — use the dedicated
 * `/activate`, `/suspend`, and `/archive` endpoints.
 */
final class UpdateDispatchPoolRequest
{
    public function __construct(
        public readonly ?string $name = null,
        public readonly ?string $description = null,
        public readonly ?int $rateLimit = null,
        public readonly ?int $concurrency = null,
    ) {}

    /**
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        $payload = [];
        if ($this->name !== null) {
            $payload['name'] = $this->name;
        }
        if ($this->description !== null) {
            $payload['description'] = $this->description;
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
