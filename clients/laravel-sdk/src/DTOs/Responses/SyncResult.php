<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Responses;

/**
 * Result of any application-scoped sync endpoint
 * (POST /api/applications/{appCode}/<resource>/sync).
 *
 * Used for roles, event types, subscriptions, dispatch pools, and
 * principals — all five share this shape.
 */
final class SyncResult
{
    /**
     * @param string[] $syncedCodes Codes (or equivalent identifiers) for the
     *                              rows that were created/updated
     */
    public function __construct(
        public readonly string $applicationCode,
        public readonly int $created,
        public readonly int $updated,
        public readonly int $deleted,
        public readonly array $syncedCodes,
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        /** @var string[] $syncedCodes */
        $syncedCodes = $data['syncedCodes'] ?? [];
        return new self(
            applicationCode: (string) ($data['applicationCode'] ?? ''),
            created: (int) ($data['created'] ?? 0),
            updated: (int) ($data['updated'] ?? 0),
            deleted: (int) ($data['deleted'] ?? 0),
            syncedCodes: $syncedCodes,
        );
    }
}
