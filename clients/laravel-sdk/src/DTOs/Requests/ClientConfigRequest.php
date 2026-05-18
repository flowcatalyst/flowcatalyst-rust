<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

/**
 * Per-client configuration body — used to enable/disable an application
 * for a client or override its base URL / config.
 */
final class ClientConfigRequest
{
    /**
     * @param array<string, mixed>|null $config
     */
    public function __construct(
        public readonly ?bool $enabled = null,
        public readonly ?string $baseUrlOverride = null,
        public readonly ?array $config = null,
    ) {}

    /**
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        $out = [];
        if ($this->enabled !== null) {
            $out['enabled'] = $this->enabled;
        }
        if ($this->baseUrlOverride !== null) {
            $out['baseUrlOverride'] = $this->baseUrlOverride;
        }
        if ($this->config !== null) {
            $out['config'] = $this->config;
        }
        return $out;
    }
}
