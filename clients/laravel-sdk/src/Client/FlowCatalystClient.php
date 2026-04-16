<?php

declare(strict_types=1);

namespace FlowCatalyst\Client;

use FlowCatalyst\Client\Auth\OidcTokenManager;
use FlowCatalyst\Client\Auth\TokenProviderInterface;
use FlowCatalyst\Client\Auth\UserTokenProvider;
use FlowCatalyst\Client\Resources\Applications;
use FlowCatalyst\Client\Resources\Clients;
use FlowCatalyst\Client\Resources\DispatchPools;
use FlowCatalyst\Client\Resources\EventTypes;
use FlowCatalyst\Client\Resources\Me;
use FlowCatalyst\Client\Resources\Permissions;
use FlowCatalyst\Client\Resources\Principals;
use FlowCatalyst\Client\Resources\Roles;
use FlowCatalyst\Client\Resources\Connections;
use FlowCatalyst\Client\Resources\Subscriptions;
use FlowCatalyst\Exceptions\AuthenticationException;
use FlowCatalyst\Exceptions\FlowCatalystException;
use FlowCatalyst\Exceptions\ValidationException;
use FlowCatalyst\Generated\Client as GeneratedClient;
use GuzzleHttp\Client;
use GuzzleHttp\Exception\GuzzleException;
use GuzzleHttp\Exception\RequestException;

class FlowCatalystClient
{
    private Client $httpClient;
    private TokenProviderInterface $tokenProvider;
    private ?GeneratedClient $generatedClient = null;
    private ?EventTypes $eventTypes = null;
    private ?Subscriptions $subscriptions = null;
    private ?DispatchPools $dispatchPools = null;
    private ?Roles $roles = null;
    private ?Permissions $permissions = null;
    private ?Applications $applications = null;
    private ?Clients $clients = null;
    private ?Principals $principals = null;
    private ?Connections $connections = null;
    private ?Me $me = null;

    /**
     * Create a new FlowCatalyst client.
     *
     * @param TokenProviderInterface|OidcTokenManager $tokenProvider Token provider for authentication
     * @param string $baseUrl Base URL of the FlowCatalyst API
     * @param int $timeout Request timeout in seconds
     * @param int $retryAttempts Number of retry attempts for transient errors
     * @param int $retryDelay Base delay between retries in milliseconds
     */
    public function __construct(
        TokenProviderInterface|OidcTokenManager $tokenProvider,
        private readonly string $baseUrl,
        private readonly int $timeout = 30,
        private readonly int $retryAttempts = 3,
        private readonly int $retryDelay = 100
    ) {
        $this->tokenProvider = $tokenProvider;
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
     * @param int $retryAttempts Number of retry attempts for transient errors
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
     * Make an authenticated API request.
     *
     * @throws FlowCatalystException
     * @throws AuthenticationException
     * @throws ValidationException
     */
    public function request(string $method, string $endpoint, array $options = []): array
    {
        $attempt = 0;
        $lastException = null;

        while ($attempt < $this->retryAttempts) {
            try {
                return $this->doRequest($method, $endpoint, $options, $attempt > 0);
            } catch (AuthenticationException $e) {
                // Don't retry auth failures
                throw $e;
            } catch (ValidationException $e) {
                // Don't retry validation errors
                throw $e;
            } catch (FlowCatalystException $e) {
                $lastException = $e;
                $attempt++;

                if ($attempt < $this->retryAttempts) {
                    usleep($this->retryDelay * 1000 * $attempt); // Exponential backoff
                }
            }
        }

        throw $lastException ?? new FlowCatalystException('Request failed after retries');
    }

    /**
     * Perform the actual HTTP request.
     */
    private function doRequest(string $method, string $endpoint, array $options, bool $isRetry): array
    {
        $token = $isRetry
            ? $this->tokenProvider->refreshToken()
            : $this->tokenProvider->getAccessToken();

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
            $statusCode = $response->getStatusCode();
            $body = (string) $response->getBody();
            $data = json_decode($body, true) ?? [];

            // Handle different status codes.
            //
            // Platform error responses look like:
            //   { "error": "<machine code>", "message": "<human text>" }
            // Surface the human message (falling back to the code) so callers
            // get "Cannot reset password for OIDC-authenticated users" instead
            // of "DUPLICATE".
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
                throw new FlowCatalystException(
                    $data['message'] ?? $data['error'] ?? "Client error: {$statusCode}",
                    $statusCode,
                    null,
                    $data
                );
            }

            if ($statusCode >= 500) {
                throw new FlowCatalystException(
                    $data['message'] ?? $data['error'] ?? "Server error: {$statusCode}",
                    $statusCode,
                    null,
                    $data
                );
            }

            return $data;
        } catch (RequestException $e) {
            throw new FlowCatalystException(
                'Request failed: ' . $e->getMessage(),
                $e->getCode(),
                $e
            );
        } catch (GuzzleException $e) {
            throw new FlowCatalystException(
                'HTTP client error: ' . $e->getMessage(),
                0,
                $e
            );
        }
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
