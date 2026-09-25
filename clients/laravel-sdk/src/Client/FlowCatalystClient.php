<?php

declare(strict_types=1);

namespace FlowCatalyst\Client;

use FlowCatalyst\Client\Auth\OidcTokenManager;
use FlowCatalyst\Client\Auth\TokenProviderInterface;
use FlowCatalyst\Client\Auth\UserTokenProvider;
use FlowCatalyst\Client\Resources\Applications;
use FlowCatalyst\Client\Resources\AuditLogs;
use FlowCatalyst\Client\Resources\Clients;
use FlowCatalyst\Client\Resources\DispatchPools;
use FlowCatalyst\Client\Resources\EventTypes;
use FlowCatalyst\Client\Resources\Me;
use FlowCatalyst\Client\Resources\Permissions;
use FlowCatalyst\Client\Resources\Principals;
use FlowCatalyst\Client\Resources\Processes;
use FlowCatalyst\Client\Resources\Roles;
use FlowCatalyst\Client\Resources\Connections;
use FlowCatalyst\Client\Resources\Router;
use FlowCatalyst\Client\Resources\ScheduledJobs;
use FlowCatalyst\Client\Resources\Subscriptions;
use FlowCatalyst\Exceptions\AuthenticationException;
use FlowCatalyst\Exceptions\FlowCatalystException;
use FlowCatalyst\Exceptions\ValidationException;
use FlowCatalyst\Generated\Client as GeneratedClient;
use FlowCatalyst\Sync\DefinitionSynchronizer;
use GuzzleHttp\Client;
use GuzzleHttp\Exception\GuzzleException;
use GuzzleHttp\Exception\RequestException;
use Psr\Http\Message\ResponseInterface;

class FlowCatalystClient
{
    private Client $httpClient;
    private TokenProviderInterface $tokenProvider;
    private readonly string $baseUrl;
    private ?GeneratedClient $generatedClient = null;
    private ?EventTypes $eventTypes = null;
    private ?Subscriptions $subscriptions = null;
    private ?DispatchPools $dispatchPools = null;
    private ?Roles $roles = null;
    private ?Permissions $permissions = null;
    private ?Applications $applications = null;
    private ?Clients $clients = null;
    private ?Principals $principals = null;
    private ?Processes $processes = null;
    private ?Connections $connections = null;
    private ?Me $me = null;
    private ?Router $router = null;
    private ?ScheduledJobs $scheduledJobs = null;
    private ?AuditLogs $auditLogs = null;
    private ?DefinitionSynchronizer $definitions = null;

    /**
     * Create a new FlowCatalyst client.
     *
     * Both $tokenProvider and $baseUrl fall back to config when omitted, so the
     * container can autowire this class. The union type was dropped to a plain
     * interface because Laravel's container cannot autowire a union-typed
     * parameter (it can't pick which member to resolve), which silently broke
     * resolution when this client was resolved without an explicit binding.
     *
     * @param TokenProviderInterface|null $tokenProvider Token provider for authentication (defaults to a config-driven OidcTokenManager)
     * @param string|null $baseUrl Base URL of the FlowCatalyst API (defaults to config('flowcatalyst.base_url'))
     * @param int $timeout Request timeout in seconds
     * @param int $retryAttempts Number of retry attempts for transient failures: HTTP 408, 429,
     *                            502, 503, 504, and network/transport failures (connect errors,
     *                            timeouts). Any other status (403, 404, 422, other 4xx, 500, 501,
     *                            505) is thrown on the first attempt and never retried.
     * @param int $retryDelay Base delay between retries in milliseconds
     */
    public function __construct(
        ?TokenProviderInterface $tokenProvider = null,
        ?string $baseUrl = null,
        private readonly int $timeout = 30,
        private readonly int $retryAttempts = 3,
        private readonly int $retryDelay = 100,
        private readonly ?string $routerBaseUrl = null
    ) {
        $this->tokenProvider = $tokenProvider ?? new OidcTokenManager();
        $this->baseUrl = $baseUrl ?? (string) config('flowcatalyst.base_url', '');
        $this->httpClient = new Client([
            'base_uri' => rtrim($this->baseUrl, '/'),
            'timeout' => $this->timeout,
            'http_errors' => false,
            // strict: true preserves the request method across 3xx redirects.
            // Guzzle's default follows 301/302 with GET (per RFC 7231), so a
            // POST through an intermediary that normalises hostnames/paths
            // silently becomes a GET — the platform then returns 405 on
            // write-only routes.
            'allow_redirects' => ['max' => 5, 'strict' => true],
        ]);
    }

    /**
     * Create a client with a user access token.
     *
     * Use this when you already have a user access token (e.g., from OIDC login)
     * and want to make API calls on behalf of that user.
     *
     * @param string|\Closure(): string $token The access token or a callable that returns it
     * @param string $baseUrl Base URL of the FlowCatalyst API
     * @param int $timeout Request timeout in seconds
     * @param int $retryAttempts Number of retry attempts for transient failures: HTTP 408, 429,
     *                            502, 503, 504, and network/transport failures (connect errors,
     *                            timeouts). Any other status (403, 404, 422, other 4xx, 500, 501,
     *                            505) is thrown on the first attempt and never retried.
     * @param int $retryDelay Base delay between retries in milliseconds
     */
    public static function withUserToken(
        string|\Closure $token,
        string $baseUrl,
        int $timeout = 30,
        int $retryAttempts = 3,
        int $retryDelay = 100
    ): self {
        return new self(
            new UserTokenProvider($token),
            $baseUrl,
            $timeout,
            $retryAttempts,
            $retryDelay
        );
    }

    /**
     * Get the Event Types resource.
     */
    public function eventTypes(): EventTypes
    {
        return $this->eventTypes ??= new EventTypes($this);
    }

    /**
     * Get the Subscriptions resource.
     */
    public function subscriptions(): Subscriptions
    {
        return $this->subscriptions ??= new Subscriptions($this);
    }

    /**
     * Get the Connections resource.
     */
    public function connections(): Connections
    {
        return $this->connections ??= new Connections($this);
    }

    /**
     * Get the Dispatch Pools resource.
     */
    public function dispatchPools(): DispatchPools
    {
        return $this->dispatchPools ??= new DispatchPools($this);
    }

    /**
     * Get the Roles resource.
     */
    public function roles(): Roles
    {
        return $this->roles ??= new Roles($this);
    }

    /**
     * Get the Permissions resource.
     */
    public function permissions(): Permissions
    {
        return $this->permissions ??= new Permissions($this);
    }

    /**
     * Get the Applications resource.
     */
    public function applications(): Applications
    {
        return $this->applications ??= new Applications($this);
    }

    /**
     * Get the Clients resource.
     */
    public function clients(): Clients
    {
        return $this->clients ??= new Clients($this);
    }

    /**
     * Get the Principals resource.
     */
    public function principals(): Principals
    {
        return $this->principals ??= new Principals($this);
    }

    /**
     * Get the Processes resource (process documentation: CRUD + sync).
     */
    public function processes(): Processes
    {
        return $this->processes ??= new Processes($this);
    }

    /**
     * Get the Me resource (user-scoped access to clients and applications).
     *
     * Use this when making requests on behalf of a user to get only
     * the resources they have access to based on their scope.
     */
    public function me(): Me
    {
        return $this->me ??= new Me($this);
    }

    /**
     * Get the Router monitoring resource.
     *
     * Provides presence checks against the message router's in-pipeline
     * map (single id and batch). Talks to the router URL configured at
     * construction; falls back to the platform `baseUrl` when no router
     * URL is set.
     */
    public function router(): Router
    {
        return $this->router ??= new Router($this);
    }

    /**
     * Get the Scheduled Jobs resource (CRUD + state transitions + history
     * reads + the SDK callback paths used by `ScheduledJobRunner`).
     */
    public function scheduledJobs(): ScheduledJobs
    {
        return $this->scheduledJobs ??= new ScheduledJobs($this);
    }

    /**
     * Read-only queries against the platform's audit-log table.
     */
    public function auditLogs(): AuditLogs
    {
        return $this->auditLogs ??= new AuditLogs($this);
    }

    /**
     * Bulk synchronizer — push a `SyncDefinitionSet` (roles, event types,
     * subscriptions, dispatch pools, principals, processes) for a single
     * application in one orchestrated call. Mirrors the TypeScript SDK's
     * `client.definitions()`.
     */
    public function definitions(): DefinitionSynchronizer
    {
        return $this->definitions ??= new DefinitionSynchronizer($this);
    }

    /**
     * The base URL the router resource should target. Returns the
     * configured `routerBaseUrl` if set, otherwise the platform `baseUrl`.
     */
    public function getRouterBaseUrl(): string
    {
        return rtrim($this->routerBaseUrl ?? $this->baseUrl, '/');
    }

    /**
     * Get the JanePHP generated API client.
     */
    public function generated(): GeneratedClient
    {
        return $this->generatedClient ??= GeneratedClientFactory::create(
            $this->tokenProvider,
            $this->baseUrl
        );
    }

    /**
     * Statuses considered transient and eligible for backoff retry. Every
     * other status (403, 404, 422, other 4xx, 500, 501, 505) is thrown on
     * the first attempt. Mirrors the TypeScript and Java SDKs' retry sets.
     */
    private const RETRYABLE_STATUSES = [408, 429, 502, 503, 504];

    /**
     * Make an authenticated API request.
     *
     * A normal attempt uses the token provider's cached access token. Only a
     * 401 triggers a one-shot {@see TokenProviderInterface::refreshToken()}
     * on the first attempt, and only when the provider is an
     * {@see OidcTokenManager} (a plain user-token provider has nothing
     * useful to refresh); a second 401 surfaces as an
     * {@see AuthenticationException}. Independently, HTTP 408/429/502/503/504
     * and network/transport failures are retried with backoff up to
     * `retryAttempts`, without ever refreshing the token.
     *
     * @throws FlowCatalystException
     * @throws AuthenticationException
     * @throws ValidationException
     */
    public function request(string $method, string $endpoint, array $options = []): array
    {
        $canRefreshToken = $this->tokenProvider instanceof OidcTokenManager;
        $token = $this->tokenProvider->getAccessToken();
        $attempt = 0;

        while (true) {
            try {
                return $this->doRequest($method, $endpoint, $options, $token);
            } catch (AuthenticationException $e) {
                if ($canRefreshToken && $attempt === 0) {
                    $token = $this->tokenProvider->refreshToken();
                    $attempt++;
                    continue;
                }

                throw $e;
            } catch (RetryableFailure $e) {
                if ($attempt + 1 < $this->retryAttempts) {
                    $attempt++;
                    usleep($this->retryDelay * 1000 * $attempt); // Exponential backoff
                    continue;
                }

                throw $e->exception;
            }
        }
    }

    /**
     * Perform the actual HTTP request with the given (already resolved)
     * bearer token.
     */
    private function doRequest(string $method, string $endpoint, array $options, string $token): array
    {
        $options['headers'] = array_merge($options['headers'] ?? [], [
            'Authorization' => "Bearer {$token}",
            'Accept' => 'application/json',
            'Content-Type' => 'application/json',
        ]);

        // Convert body to JSON if it's an array
        if (isset($options['body']) && is_array($options['body'])) {
            $options['body'] = json_encode($options['body']);
        }

        // DEBUG: Log request details
        if (env('FLOWCATALYST_DEBUG', false)) {
            \Illuminate\Support\Facades\Log::debug("FLOWCATALYST REQUEST: {$method} {$endpoint}", [
                'body' => $options['body'] ?? $options['json'] ?? null,
            ]);
        }

        // Handle JSON body
        if (isset($options['json'])) {
            $options['body'] = json_encode($options['json']);
            unset($options['json']);
        }

        try {
            $response = $this->httpClient->request($method, $endpoint, $options);
        } catch (RequestException $e) {
            // The client is built with http_errors=false, so this only fires
            // for genuine transport failures (connect errors, timeouts).
            // Defensively check for an attached response anyway: a wrapped
            // 404 (or any status) must go through the normal status handling
            // below, never be mistaken for a network failure and retried.
            if ($e->hasResponse()) {
                return $this->handleResponse($e->getResponse());
            }

            throw new RetryableFailure(new FlowCatalystException(
                'Request failed: ' . $e->getMessage(),
                $e->getCode(),
                $e
            ));
        } catch (GuzzleException $e) {
            throw new RetryableFailure(new FlowCatalystException(
                'HTTP client error: ' . $e->getMessage(),
                0,
                $e
            ));
        }

        return $this->handleResponse($response);
    }

    /**
     * Translate an HTTP response into either the decoded payload or the
     * appropriate typed exception.
     *
     * Platform error responses look like:
     *   { "error": "<machine code>", "message": "<human text>" }
     * Surface the human message (falling back to the code) so callers
     * get "Cannot reset password for OIDC-authenticated users" instead
     * of "DUPLICATE".
     */
    private function handleResponse(ResponseInterface $response): array
    {
        $statusCode = $response->getStatusCode();
        $body = (string) $response->getBody();
        $data = json_decode($body, true) ?? [];

        if ($statusCode === 401) {
            throw AuthenticationException::tokenExpired();
        }

        if ($statusCode === 403) {
            throw new FlowCatalystException(
                $data['message'] ?? $data['error'] ?? 'Access forbidden',
                403,
                null,
                $data
            );
        }

        if ($statusCode === 404) {
            throw new FlowCatalystException(
                $data['message'] ?? $data['error'] ?? 'Resource not found',
                404,
                null,
                $data
            );
        }

        if ($statusCode === 422) {
            throw ValidationException::fromResponse($data);
        }

        if ($statusCode >= 400 && $statusCode < 500) {
            $exception = new FlowCatalystException(
                $data['message'] ?? $data['error'] ?? "Client error: {$statusCode}",
                $statusCode,
                null,
                $data
            );

            throw in_array($statusCode, self::RETRYABLE_STATUSES, true)
                ? new RetryableFailure($exception)
                : $exception;
        }

        if ($statusCode >= 500) {
            $exception = new FlowCatalystException(
                $data['message'] ?? $data['error'] ?? "Server error: {$statusCode}",
                $statusCode,
                null,
                $data
            );

            throw in_array($statusCode, self::RETRYABLE_STATUSES, true)
                ? new RetryableFailure($exception)
                : $exception;
        }

        return $data;
    }

    /**
     * Get the base URL.
     */
    public function getBaseUrl(): string
    {
        return $this->baseUrl;
    }

    /**
     * Get the token provider.
     */
    public function getTokenProvider(): TokenProviderInterface
    {
        return $this->tokenProvider;
    }

    /**
     * Get the token manager (for backward compatibility).
     *
     * @deprecated Use getTokenProvider() instead
     * @throws \RuntimeException If the token provider is not an OidcTokenManager
     */
    public function getTokenManager(): OidcTokenManager
    {
        if (!$this->tokenProvider instanceof OidcTokenManager) {
            throw new \RuntimeException(
                'getTokenManager() is only available when using OidcTokenManager. Use getTokenProvider() instead.'
            );
        }

        return $this->tokenProvider;
    }
}
