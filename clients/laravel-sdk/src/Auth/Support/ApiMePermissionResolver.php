<?php

declare(strict_types=1);

namespace FlowCatalyst\Auth\Support;

use FlowCatalyst\Auth\Contracts\PermissionResolver;
use GuzzleHttp\ClientInterface;
use Illuminate\Contracts\Cache\Repository as Cache;

/**
 * Default {@see PermissionResolver}: fetches the caller's server-resolved
 * permissions from GET /api/me and caches them keyed by md5(token). One HTTP
 * round-trip per token, then cached for `ttlSeconds`.
 *
 * Fail-closed: returns [] on any non-200 / transport error, so a caller has no
 * permissions until /api/me succeeds again. Bind a different PermissionResolver
 * (e.g. {@see \FlowCatalyst\Auth\Rbac\CataloguePermissionResolver}, or a
 * composite) if you need offline resilience.
 */
final class ApiMePermissionResolver implements PermissionResolver
{
    public function __construct(
        private readonly ClientInterface $http,
        private readonly Cache $cache,
        private readonly string $baseUrl,
        private readonly int $ttlSeconds = 60,
    ) {
    }

    public function resolve(array $roles, ?string $token = null): array
    {
        if ($token === null || $token === '') {
            return [];
        }

        $key = 'fc.me.perms.' . md5($token);

        $cached = $this->cache->get($key);
        if (is_array($cached)) {
            return $cached;
        }

        $permissions = $this->fetch($token);
        if ($permissions === null) {
            return [];
        }

        $this->cache->put($key, $permissions, $this->ttlSeconds);
        return $permissions;
    }

    /** @return array<int,string>|null null on failure */
    private function fetch(string $token): ?array
    {
        try {
            $response = $this->http->request('GET', rtrim($this->baseUrl, '/') . '/api/me', [
                'headers' => [
                    'Authorization' => 'Bearer ' . $token,
                    'Accept' => 'application/json',
                ],
                'http_errors' => false,
                'timeout' => 5,
            ]);
        } catch (\Throwable) {
            return null;
        }

        if ($response->getStatusCode() !== 200) {
            return null;
        }

        $body = json_decode((string) $response->getBody(), true);
        if (! is_array($body) || ! isset($body['permissions']) || ! is_array($body['permissions'])) {
            return null;
        }

        return array_values(array_map('strval', array_filter($body['permissions'], 'is_scalar')));
    }
}
