<?php

declare(strict_types=1);

namespace FlowCatalyst\Auth\Support;

use GuzzleHttp\ClientInterface;
use Illuminate\Contracts\Cache\Repository as Cache;

/**
 * Backs the opt-in `oidc.check_revocation` feature: asks
 * GET /api/principals/{id}/version whether the principal (or a role it
 * holds) changed after a given moment — typically the stored access
 * token's `iat` — so {@see \FlowCatalyst\Auth\Http\Middleware\AuthenticateFc}
 * can force a refresh sooner than the token's own natural expiry.
 *
 * Cached per principal (not per token — many tokens share one answer),
 * mirroring {@see ApiMePermissionResolver}'s cache-on-success-only shape.
 *
 * Fails OPEN (returns false — "not stale") on any transport/parse error or
 * non-200. This check is a bonus on top of the ALREADY-valid, unexpired
 * access token AuthenticateFc holds; a transient platform blip must not
 * force every session into a refresh loop. The mandatory exp-based cap
 * (unaffected by this check) is what bounds worst-case staleness.
 */
final class RevocationChecker
{
    /**
     * Bounds on the configured cache TTL — how long a "did this principal
     * change" answer is trusted before the next request re-asks the
     * platform. Clamped (not just documented) so a misconfigured value
     * can't silently defeat either end of the tradeoff: too low re-hits the
     * platform on effectively every request; too high makes the opt-in
     * revocation check meaningless (no better than waiting for natural
     * token expiry).
     */
    private const MIN_TTL_SECONDS = 5;
    private const MAX_TTL_SECONDS = 600;

    private readonly int $ttlSeconds;

    public function __construct(
        private readonly ClientInterface $http,
        private readonly Cache $cache,
        private readonly string $baseUrl,
        int $ttlSeconds = 60,
    ) {
        $this->ttlSeconds = max(self::MIN_TTL_SECONDS, min(self::MAX_TTL_SECONDS, $ttlSeconds));
    }

    /**
     * True if $principalId's version is newer than $sinceUnixTimestamp.
     */
    public function isStale(string $principalId, string $bearerToken, int $sinceUnixTimestamp): bool
    {
        $key = 'fc.principal.version.' . $principalId;

        $cachedAt = $this->cache->get($key);
        if (is_int($cachedAt)) {
            return $cachedAt > $sinceUnixTimestamp;
        }

        $updatedAt = $this->fetch($principalId, $bearerToken);
        if ($updatedAt === null) {
            return false;
        }

        $this->cache->put($key, $updatedAt, $this->ttlSeconds);
        return $updatedAt > $sinceUnixTimestamp;
    }

    private function fetch(string $principalId, string $bearerToken): ?int
    {
        try {
            $response = $this->http->request(
                'GET',
                rtrim($this->baseUrl, '/') . '/api/principals/' . $principalId . '/version',
                [
                    'headers' => [
                        'Authorization' => 'Bearer ' . $bearerToken,
                        'Accept' => 'application/json',
                    ],
                    'http_errors' => false,
                    'timeout' => 5,
                ],
            );
        } catch (\Throwable) {
            return null;
        }

        if ($response->getStatusCode() !== 200) {
            return null;
        }

        $body = json_decode((string) $response->getBody(), true);
        $updatedAt = is_array($body) ? ($body['updatedAt'] ?? null) : null;
        if (!is_string($updatedAt)) {
            return null;
        }

        $ts = strtotime($updatedAt);
        return $ts === false ? null : $ts;
    }
}
