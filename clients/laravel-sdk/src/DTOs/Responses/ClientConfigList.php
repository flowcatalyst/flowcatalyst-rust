<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Responses;

use FlowCatalyst\DTOs\ClientConfig;

/**
 * Wraps `GET /api/applications/{id}/clients` — the list of per-client
 * configurations for an application.
 */
final class ClientConfigList
{
    /**
     * @param ClientConfig[] $clientConfigs
     */
    public function __construct(
        public readonly array $clientConfigs,
        public readonly ?int $total = null,
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        /** @var array<int, array<string, mixed>> $rows */
        $rows = $data['clientConfigs'] ?? [];
        $configs = array_map(static fn(array $row) => ClientConfig::fromArray($row), $rows);
        return new self(
            clientConfigs: $configs,
            total: isset($data['total']) ? (int) $data['total'] : null,
        );
    }
}
