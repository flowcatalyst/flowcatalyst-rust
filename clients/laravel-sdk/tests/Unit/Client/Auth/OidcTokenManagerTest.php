<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Client\Auth;

use FlowCatalyst\Client\Auth\OidcTokenManager;
use GuzzleHttp\Client;
use GuzzleHttp\Handler\MockHandler;
use GuzzleHttp\HandlerStack;
use GuzzleHttp\Psr7\Response;
use Illuminate\Cache\ArrayStore;
use Illuminate\Cache\Repository;
use PHPUnit\Framework\TestCase;

final class OidcTokenManagerTest extends TestCase
{
    /**
     * @param array<int, Response> $responses
     */
    private function managerWithMockedHttp(array $responses): OidcTokenManager
    {
        $manager = new OidcTokenManager(
            baseUrl: 'https://fc.test',
            clientId: 'client-id',
            clientSecret: 'client-secret',
            tokenUrl: 'https://fc.test/oauth/token',
            cache: new Repository(new ArrayStore()),
            cacheKey: 'test_flowcatalyst_token',
        );

        $httpClient = new Client([
            'handler' => HandlerStack::create(new MockHandler($responses)),
        ]);

        $prop = new \ReflectionProperty(OidcTokenManager::class, 'httpClient');
        $prop->setAccessible(true);
        $prop->setValue($manager, $httpClient);

        return $manager;
    }

    /**
     * T7: expires_in = 30 must still be cached. TTL = max(30-60, 30*0.9) =
     * max(-30, 27) = 27s, floored at 1. With only ONE response queued, a
     * second getAccessToken() that hits /oauth/token again would exhaust
     * the MockHandler and throw — proving the assertion pins caching, not
     * just the return value.
     *
     * Mutant: revert the TTL formula to `expires_in - 60` (= -30, i.e. an
     * expired/negative TTL that caches nothing) — this test then fails with
     * an "empty mock queue" exception from the second fetch.
     */
    public function test_short_lived_token_is_still_cached(): void
    {
        $manager = $this->managerWithMockedHttp([
            new Response(200, [], json_encode(['access_token' => 'tok-1', 'expires_in' => 30])),
        ]);

        $first = $manager->getAccessToken();
        $second = $manager->getAccessToken();

        $this->assertSame('tok-1', $first);
        $this->assertSame('tok-1', $second);
    }

    /**
     * A normal long-lived token keeps behaving exactly as before: TTL is
     * expires_in - 60 (3540s for a 3600s token), not the 90% fallback.
     */
    public function test_normal_token_uses_the_sixty_second_buffer(): void
    {
        $manager = $this->managerWithMockedHttp([
            new Response(200, [], json_encode(['access_token' => 'tok-1', 'expires_in' => 3600])),
        ]);

        $manager->getAccessToken();

        $expiry = new \ReflectionProperty(OidcTokenManager::class, 'cachedExpiry');
        $expiry->setAccessible(true);
        $value = $expiry->getValue($manager);

        // Allow a small margin for wall-clock drift between fetch and assertion.
        $this->assertGreaterThanOrEqual(time() + 3540 - 2, $value);
        $this->assertLessThanOrEqual(time() + 3540 + 2, $value);
    }
}
