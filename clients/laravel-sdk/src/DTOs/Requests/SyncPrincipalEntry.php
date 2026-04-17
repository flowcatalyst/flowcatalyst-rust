<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

/**
 * One entry in the POST /api/applications/{appCode}/principals/sync payload.
 *
 * Sync semantics (declarative):
 * - Roles are prefixed with the application code server-side.
 * - If `removeUnlisted` is true on the enclosing call, SDK-synced roles on
 *   unlisted principals are removed.
 */
final class SyncPrincipalEntry
{
    /**
     * @param string[] $roles Role codes (without the app prefix — the platform adds it)
     */
    public function __construct(
        public readonly string $email,
        public readonly string $name,
        public readonly array $roles = [],
        public readonly ?bool $active = null,
    ) {}

    /**
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        $payload = [
            'email' => $this->email,
            'name' => $this->name,
            'roles' => $this->roles,
        ];
        if ($this->active !== null) {
            $payload['active'] = $this->active;
        }
        return $payload;
    }
}
