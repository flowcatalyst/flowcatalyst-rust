<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Client;

use FlowCatalyst\Client\Auth\OidcTokenManager;
use FlowCatalyst\Client\Auth\TokenProviderInterface;
use FlowCatalyst\Client\Auth\UserTokenProvider;
use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\Exceptions\AuthenticationException;
use FlowCatalyst\Exceptions\FlowCatalystException;
use GuzzleHttp\Client;
use GuzzleHttp\Exception\ConnectException;
use GuzzleHttp\Handler\MockHandler;
use GuzzleHttp\HandlerStack;
use GuzzleHttp\Middleware;
use GuzzleHttp\Psr7\Request;
use GuzzleHttp\Psr7\Response;
use PHPUnit\Framework\TestCase;

/**
 * Pins the retry/refresh contract in FlowCatalystClient::request():
 *  - only 401 triggers a one-shot token refresh (client-credentials mode only)
 *  - only 408/429/502/503/504 and network failures are retried with backoff
 *  - retries never refresh the token; refresh never retries the status set
 */
final class FlowCatalystClientTest extends TestCase
{
    /**
     * @param array<int, Response|\Throwable> $responses
     */
    private function clientWithMockedHttp(
        array $responses,
        TokenProviderInterface $tokenProvider,
        \ArrayObject $history,
        int $retryAttempts = 3
    ): FlowCatalystClient {
        $client = new FlowCatalystClient(
            tokenProvider: $tokenProvider,
            baseUrl: 'https://fc.test',
            retryAttempts: $retryAttempts,
            retryDelay: 1,
        );

        $stack = HandlerStack::create(new MockHandler($responses));
        $stack->push(Middleware::history($history));
        $httpClient = new Client([
            'handler' => $stack,
            'http_errors' => false,
            'base_uri' => 'https://fc.test',
        ]);

        $prop = new \ReflectionProperty(FlowCatalystClient::class, 'httpClient');
        $prop->setAccessible(true);
        $prop->setValue($client, $httpClient);

        return $client;
    }

    private function authHeaderAt(\ArrayObject $history, int $index): string
    {
        $entries = iterator_to_array($history);
        return $entries[$index]['request']->getHeaderLine('Authorization');
    }

    /**
     * T1: a 404 costs exactly one HTTP request and zero token refreshes.
     * Mutant: restore "retry every FlowCatalystException up to retryAttempts"
     * — then this test fails because the history has 3 requests, not 1.
     */
    public function test_a_404_makes_exactly_one_request_and_never_refreshes(): void
    {
        $tokenProvider = new CountingTokenProvider('tok-1');
        $history = new \ArrayObject();
        $client = $this->clientWithMockedHttp(
            [new Response(404, [], json_encode(['message' => 'not found']))],
            $tokenProvider,
            $history,
        );

        try {
            $client->request('GET', '/api/thing/missing');
            $this->fail('expected FlowCatalystException');
        } catch (FlowCatalystException $e) {
            $this->assertSame(404, $e->getCode());
        }

        $this->assertCount(1, $history);
        $this->assertSame(1, $tokenProvider->getCalls);
        $this->assertSame(0, $tokenProvider->refreshCalls);
    }

    /**
     * T2: 403 and 500 are each thrown on the first attempt, no refresh.
     * Mutant: add 500 to the retryable status set — this test then fails
     * because the 500 case makes more than one request.
     */
    public function test_403_and_500_are_thrown_on_first_attempt(): void
    {
        foreach ([403, 500] as $status) {
            $tokenProvider = new CountingTokenProvider('tok-1');
            $history = new \ArrayObject();
            $client = $this->clientWithMockedHttp(
                [new Response($status, [], json_encode(['message' => 'boom']))],
                $tokenProvider,
                $history,
            );

            try {
                $client->request('GET', '/api/thing');
                $this->fail("expected FlowCatalystException for status {$status}");
            } catch (FlowCatalystException $e) {
                $this->assertSame($status, $e->getCode());
            }

            $this->assertCount(1, $history, "status {$status} must not retry");
            $this->assertSame(0, $tokenProvider->refreshCalls, "status {$status} must not refresh");
        }
    }

    /**
     * T3: a 503 then a 200 retries once and succeeds, using the SAME token
     * on both attempts (no refresh).
     * Mutant: refresh the token before a retry — this test then fails
     * because the two Authorization headers differ.
     */
    public function test_503_then_200_retries_without_refreshing_token(): void
    {
        $tokenProvider = new CountingTokenProvider('tok-1');
        $history = new \ArrayObject();
        $client = $this->clientWithMockedHttp(
            [
                new Response(503, [], json_encode(['message' => 'unavailable'])),
                new Response(200, [], json_encode(['ok' => true])),
            ],
            $tokenProvider,
            $history,
        );

        $result = $client->request('GET', '/api/thing');

        $this->assertSame(['ok' => true], $result);
        $this->assertCount(2, $history);
        $this->assertSame(0, $tokenProvider->refreshCalls);
        $this->assertSame(
            $this->authHeaderAt($history, 0),
            $this->authHeaderAt($history, 1),
            'both attempts must carry the same token'
        );
    }

    /**
     * T4: a 401 then a 200 refreshes exactly once and resends with the new
     * token.
     * Mutant: don't retry 401 at all — this test then fails because the
     * client throws AuthenticationException instead of returning data, and
     * the history only has 1 request.
     */
    public function test_401_then_200_refreshes_once_and_resends_with_new_token(): void
    {
        $tokenProvider = new CountingOidcTokenManager('tok-1', 'tok-2');
        $history = new \ArrayObject();
        $client = $this->clientWithMockedHttp(
            [
                new Response(401, [], json_encode(['message' => 'expired'])),
                new Response(200, [], json_encode(['ok' => true])),
            ],
            $tokenProvider,
            $history,
        );

        $result = $client->request('GET', '/api/thing');

        $this->assertSame(['ok' => true], $result);
        $this->assertCount(2, $history);
        $this->assertSame(1, $tokenProvider->refreshCalls);
        $this->assertSame('Bearer tok-1', $this->authHeaderAt($history, 0));
        $this->assertSame('Bearer tok-2', $this->authHeaderAt($history, 1));
    }

    /**
     * T5: 401 then 401 surfaces AuthenticationException after exactly 2
     * requests and exactly 1 refresh (the second 401 is not retried again).
     * Mutant: refresh on every 401 attempt (no one-shot guard) — this test
     * then fails because refreshCalls would exceed 1 (or the request loop
     * would spin trying more than 2 requests).
     */
    public function test_401_then_401_surfaces_authentication_exception(): void
    {
        $tokenProvider = new CountingOidcTokenManager('tok-1', 'tok-2');
        $history = new \ArrayObject();
        $client = $this->clientWithMockedHttp(
            [
                new Response(401, [], json_encode(['message' => 'expired'])),
                new Response(401, [], json_encode(['message' => 'expired'])),
            ],
            $tokenProvider,
            $history,
        );

        try {
            $client->request('GET', '/api/thing');
            $this->fail('expected AuthenticationException');
        } catch (AuthenticationException $e) {
            // expected
        }

        $this->assertCount(2, $history);
        $this->assertSame(1, $tokenProvider->refreshCalls);
    }

    /**
     * T6: a network failure (ConnectException) then a 200 retries and
     * succeeds.
     * Mutant: stop retrying network errors — this test then fails because
     * the ConnectException propagates instead of being retried.
     */
    public function test_network_failure_then_200_retries_and_succeeds(): void
    {
        $tokenProvider = new CountingTokenProvider('tok-1');
        $history = new \ArrayObject();
        $client = $this->clientWithMockedHttp(
            [
                new ConnectException('connection refused', new Request('GET', 'https://fc.test/api/thing')),
                new Response(200, [], json_encode(['ok' => true])),
            ],
            $tokenProvider,
            $history,
        );

        $result = $client->request('GET', '/api/thing');

        $this->assertSame(['ok' => true], $result);
        $this->assertCount(2, $history);
        $this->assertSame(0, $tokenProvider->refreshCalls);
    }

    /**
     * Bonus (not one of T1-T7, but pins item 1's "no-refresh provider"
     * clause): a plain UserTokenProvider is not an OidcTokenManager, so a
     * 401 is never retried with a refresh — it surfaces immediately after
     * exactly one request.
     */
    public function test_user_token_provider_never_refreshes_on_401(): void
    {
        $history = new \ArrayObject();
        $client = $this->clientWithMockedHttp(
            [new Response(401, [], json_encode(['message' => 'expired']))],
            new UserTokenProvider('tok-1'),
            $history,
        );

        try {
            $client->request('GET', '/api/thing');
            $this->fail('expected AuthenticationException');
        } catch (AuthenticationException $e) {
            // expected
        }

        $this->assertCount(1, $history);
    }
}

/**
 * Simple counting fake — not an OidcTokenManager, so FlowCatalystClient
 * never treats it as refreshable (matching a user-token/no-refresh
 * provider's shape without depending on UserTokenProvider's own behaviour).
 */
final class CountingTokenProvider implements TokenProviderInterface
{
    public int $getCalls = 0;
    public int $refreshCalls = 0;

    public function __construct(private readonly string $token)
    {
    }

    public function getAccessToken(): string
    {
        $this->getCalls++;
        return $this->token;
    }

    public function refreshToken(): string
    {
        $this->refreshCalls++;
        return $this->token;
    }
}

/**
 * Counting fake that IS an OidcTokenManager (by subclassing), so
 * FlowCatalystClient's `canRefreshToken` gate treats it as refreshable,
 * without making any real HTTP calls to an OAuth token endpoint.
 */
final class CountingOidcTokenManager extends OidcTokenManager
{
    public int $getCalls = 0;
    public int $refreshCalls = 0;
    private string $current;

    public function __construct(string $initial, private readonly string $refreshed)
    {
        // Deliberately does not call parent::__construct(): the parent's
        // config/cache wiring is irrelevant to this fake, which overrides
        // every public method the client calls.
        $this->current = $initial;
    }

    public function getAccessToken(): string
    {
        $this->getCalls++;
        return $this->current;
    }

    public function refreshToken(): string
    {
        $this->refreshCalls++;
        $this->current = $this->refreshed;
        return $this->current;
    }
}
