<?php

declare(strict_types=1);

namespace FlowCatalyst\Auth\Support;

use FlowCatalyst\Auth\Contracts\PermissionResolver;

/**
 * Stateless {@see PermissionResolver}: reads the principal's granted permissions
 * straight off the access token's `scope` claim (space-delimited), exactly as
 * the platform mints them. Zero HTTP, zero database — the token IS the source
 * of truth.
 *
 * Pair with the `fc-token` (Bearer) or `fc-session` guard for a fully stateless,
 * Spatie-free authorization model: no users row, no permission tables. When
 * platform permissions change, the token goes stale until refreshed — use the
 * refresh-token flow (or a short token TTL) to pick up changes.
 *
 * The signature is NOT re-verified here — the guard already validated the token
 * (via {@see AccessTokenValidator}) before resolution, so reading its claims is
 * safe. Returns `[]` when there is no token / no scope claim.
 */
final class TokenScopePermissionResolver implements PermissionResolver
{
    /**
     * @param array<int,string> $roles Unused — permissions come from the token.
     * @return array<int,string>
     */
    public function resolve(array $roles, ?string $token = null): array
    {
        if ($token === null || $token === '') {
            return [];
        }

        $claims = $this->decodeClaims($token);
        if ($claims === null) {
            return [];
        }

        // Granted permissions ride the OAuth `scope` claim (space-delimited).
        $scope = $claims['scope'] ?? null;
        if (is_string($scope)) {
            $parts = preg_split('/\s+/', trim($scope));
            return $parts === false ? [] : array_values(array_filter($parts, static fn ($p) => $p !== ''));
        }
        if (is_array($scope)) {
            return array_values(array_filter($scope, 'is_string'));
        }

        // Some tokens may carry an explicit `permissions` array instead.
        $permissions = $claims['permissions'] ?? null;
        if (is_array($permissions)) {
            return array_values(array_filter($permissions, 'is_string'));
        }

        return [];
    }

    /**
     * Decode (without verifying) a JWT's payload claims.
     *
     * @return array<string, mixed>|null
     */
    private function decodeClaims(string $token): ?array
    {
        $parts = explode('.', $token);
        if (count($parts) !== 3) {
            return null;
        }

        $payload = $parts[1];
        $remainder = strlen($payload) % 4;
        if ($remainder > 0) {
            $payload .= str_repeat('=', 4 - $remainder);
        }

        $json = base64_decode(strtr($payload, '-_', '+/'), true);
        if ($json === false) {
            return null;
        }

        $decoded = json_decode($json, true);

        return is_array($decoded) ? $decoded : null;
    }
}
