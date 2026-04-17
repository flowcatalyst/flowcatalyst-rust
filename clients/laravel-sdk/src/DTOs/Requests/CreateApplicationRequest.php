<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

/**
 * Payload for POST /api/applications.
 *
 * `type` is `APPLICATION` (default) or `INTEGRATION`.
 */
final class CreateApplicationRequest
{
    public function __construct(
        public readonly string $code,
        public readonly string $name,
        public readonly ?string $description = null,
        public readonly ?string $type = null,
        public readonly ?string $defaultBaseUrl = null,
        public readonly ?string $iconUrl = null,
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
        if ($this->type !== null) {
            $payload['type'] = $this->type;
        }
        if ($this->defaultBaseUrl !== null) {
            $payload['defaultBaseUrl'] = $this->defaultBaseUrl;
        }
        if ($this->iconUrl !== null) {
            $payload['iconUrl'] = $this->iconUrl;
        }
        return $payload;
    }
}
