<?php

declare(strict_types=1);

namespace FlowCatalyst\Client\Auth;

use FlowCatalyst\Exceptions\AuthenticationException;
use GuzzleHttp\Client;
use GuzzleHttp\Exception\GuzzleException;
use Illuminate\Contracts\Cache\Repository as CacheRepository;
use Illuminate\Support\Facades\Cache;

class OidcTokenManager implements TokenProviderInterface
{
    private Client $httpClient;
    private ?string $cachedToken = null;
    private ?int $cachedExpiry = null;

    private readonly string $baseUrl;
    private readonly string $clientId;
    private readonly string $clientSecret;
    private readonly ?string $tokenUrl;
    private readonly CacheRepository $cache;
    private readonly string $cacheKey;

    /**
     * Every argument falls back to config('flowcatalyst.*') when null, so the
     * container can resolve this class by *autowiring* — e.g. when it is bound
     * via `bind(TokenProviderInterface::class, OidcTokenManager::class)` and the
     * cascade to the config-building singleton isn't in effect. Without this,
     * autowiring can't supply the scalar credentials and they arrive empty,
     * surfacing later as a misleading "missing FLOWCATALYST_CLIENT_ID" error.
     *
     * Pass arguments explicitly to override config (e.g. per-tenant credentials
     * resolved at runtime).
     */
    public function __construct(
        ?string $baseUrl = null,
        ?string $clientId = null,
        ?string $clientSecret = null,
        ?string $tokenUrl = null,
        ?CacheRepository $cache = null,
        ?string $cacheKey = null,
    ) {
        $this->baseUrl = $baseUrl ?? (string) config('flowcatalyst.base_url', '');
        $this->clientId = $clientId ?? (string) config('flowcatalyst.client_id', '');
        $this->clientSecret = $clientSecret ?? (string) config('flowcatalyst.client_secret', '');
        $this->tokenUrl = $tokenUrl ?? config('flowcatalyst.token_url');
        $this->cacheKey = $cacheKey ?? (string) config('flowcatalyst.token_cache.key', 'flowcatalyst_access_token');
        $this->cache = $cache ?? Cache::store(config('flowcatalyst.token_cache.driver'));

        $this->httpClient = new Client([
            'timeout' => 30,
            'http_errors' => false,
        ]);
    }

    /**
     * Get a valid access token, refreshing if necessary.
     *
     * @throws AuthenticationException
     */
    public function getAccessToken(): string
    {
        // Check in-memory cache first
        if ($this->cachedToken !== null && $this->cachedExpiry !== null && $this->cachedExpiry > time()) {
            return $this->cachedToken;
        }

        // Check Laravel cache
        $cached = $this->cache->get($this->cacheKey);
        if ($cached !== null && is_array($cached)) {
            $token = $cached['token'] ?? null;
            $expiry = $cached['expiry'] ?? 0;

            if ($token !== null && $expiry > time()) {
                $this->cachedToken = $token;
                $this->cachedExpiry = $expiry;
                return $token;
            }
        }

        // Fetch new token
        return $this->fetchNewToken();
    }

    /**
     * Force refresh the access token.
     *
     * @throws AuthenticationException
     */
    public function refreshToken(): string
    {
        $this->cachedToken = null;
        $this->cachedExpiry = null;
        $this->cache->forget($this->cacheKey);

        return $this->fetchNewToken();
    }

    /**
     * Check if we have valid credentials configured.
     */
    public function hasCredentials(): bool
    {
        return !empty($this->clientId) && !empty($this->clientSecret);
    }

    /**
     * Fetch a new token from the OAuth server.
     *
     * @throws AuthenticationException
     */
    private function fetchNewToken(): string
    {
        if (!$this->hasCredentials()) {
            throw AuthenticationException::missingConfiguration('FLOWCATALYST_CLIENT_ID and FLOWCATALYST_CLIENT_SECRET');
        }

        $tokenUrl = $this->tokenUrl ?? rtrim($this->baseUrl, '/') . '/oauth/token';

        try {
            $response = $this->httpClient->post($tokenUrl, [
                'form_params' => [
                    'grant_type' => 'client_credentials',
                    'client_id' => $this->clientId,
                    'client_secret' => $this->clientSecret,
                ],
                'headers' => [
                    'Accept' => 'application/json',
                ],
            ]);

            $statusCode = $response->getStatusCode();
            $body = json_decode((string) $response->getBody(), true);

            if ($statusCode === 401 || $statusCode === 403) {
                throw AuthenticationException::invalidCredentials();
            }

            if ($statusCode !== 200 || !isset($body['access_token'])) {
                $error = $body['error'] ?? $body['error_description'] ?? 'Unknown error';
                throw AuthenticationException::tokenFetchFailed($error);
            }

            $token = $body['access_token'];
            $expiresIn = $body['expires_in'] ?? 3600;

            // Cache with a 60 second buffer before expiry, but never let a
            // short-lived token (expires_in <= 60) cache for zero/negative
            // seconds and refetch on every call: fall back to 90% of the
            // token's lifetime, floored at 1 second.
            $ttl = max(max($expiresIn - 60, (int) ($expiresIn * 0.9)), 1);
            $expiry = time() + $ttl;

            // Store in Laravel cache
            $this->cache->put($this->cacheKey, [
                'token' => $token,
                'expiry' => $expiry,
            ], $ttl);

            // Store in memory
            $this->cachedToken = $token;
            $this->cachedExpiry = $expiry;

            return $token;
        } catch (GuzzleException $e) {
            throw AuthenticationException::tokenFetchFailed($e->getMessage());
        }
    }

    /**
     * Clear the cached token.
     */
    public function clearCache(): void
    {
        $this->cachedToken = null;
        $this->cachedExpiry = null;
        $this->cache->forget($this->cacheKey);
    }
}
