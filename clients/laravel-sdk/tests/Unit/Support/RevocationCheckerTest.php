<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Support;

use FlowCatalyst\Auth\Support\RevocationChecker;
use GuzzleHttp\Client;
use GuzzleHttp\Handler\MockHandler;
use GuzzleHttp\HandlerStack;
use GuzzleHttp\Psr7\Response;
use Illuminate\Cache\ArrayStore;
use Illuminate\Cache\Repository;
use PHPUnit\Framework\TestCase;

final class RevocationCheckerTest extends TestCase
{
    private function clientReturning(array $responses): Client
    {
        $mock = new MockHandler($responses);
        return new Client(['handler' => HandlerStack::create($mock)]);
    }

    private function cache(): Repository
    {
        return new Repository(new ArrayStore());
    }

    private function ttlSecondsOf(RevocationChecker $checker): int
    {
        $prop = new \ReflectionProperty(RevocationChecker::class, 'ttlSeconds');
        $prop->setAccessible(true);
        return $prop->getValue($checker);
    }

    public function test_ttl_below_minimum_is_clamped_up(): void
    {
        $checker = new RevocationChecker($this->clientReturning([]), $this->cache(), 'https://fc.test', ttlSeconds: 0);
        $this->assertSame(5, $this->ttlSecondsOf($checker));

        $negative = new RevocationChecker($this->clientReturning([]), $this->cache(), 'https://fc.test', ttlSeconds: -30);
        $this->assertSame(5, $this->ttlSecondsOf($negative));
    }

    public function test_ttl_above_maximum_is_clamped_down(): void
    {
        $checker = new RevocationChecker($this->clientReturning([]), $this->cache(), 'https://fc.test', ttlSeconds: 100_000);
        $this->assertSame(600, $this->ttlSecondsOf($checker));
    }

    public function test_ttl_within_range_is_used_verbatim(): void
    {
        $checker = new RevocationChecker($this->clientReturning([]), $this->cache(), 'https://fc.test', ttlSeconds: 120);
        $this->assertSame(120, $this->ttlSecondsOf($checker));
    }

    public function test_second_call_within_ttl_does_not_refetch(): void
    {
        $cache = $this->cache();
        // Only one response queued — a second HTTP call would throw
        // (MockHandler exhausted), proving the cache serves the second read.
        $checker = new RevocationChecker(
            $this->clientReturning([new Response(200, [], json_encode(['updatedAt' => '2026-01-01T00:00:10Z']))]),
            $cache,
            'https://fc.test',
            ttlSeconds: 60,
        );

        $first = $checker->isStale('prn_1', 'token', 100);
        $second = $checker->isStale('prn_1', 'token', 100);

        $this->assertSame($first, $second);
    }

    public function test_stale_when_updated_at_is_after_since(): void
    {
        $checker = new RevocationChecker(
            $this->clientReturning([new Response(200, [], json_encode(['updatedAt' => '2026-01-01T00:00:10Z']))]),
            $this->cache(),
            'https://fc.test',
        );

        $updatedAtEpoch = strtotime('2026-01-01T00:00:10Z');
        $this->assertTrue($checker->isStale('prn_1', 'token', $updatedAtEpoch - 5));
    }

    public function test_not_stale_when_updated_at_is_before_since(): void
    {
        $checker = new RevocationChecker(
            $this->clientReturning([new Response(200, [], json_encode(['updatedAt' => '2026-01-01T00:00:10Z']))]),
            $this->cache(),
            'https://fc.test',
        );

        $updatedAtEpoch = strtotime('2026-01-01T00:00:10Z');
        $this->assertFalse($checker->isStale('prn_1', 'token', $updatedAtEpoch + 5));
    }

    public function test_fails_open_on_non_200(): void
    {
        $checker = new RevocationChecker(
            $this->clientReturning([new Response(500)]),
            $this->cache(),
            'https://fc.test',
        );

        $this->assertFalse($checker->isStale('prn_1', 'token', 0));
    }

    public function test_fails_open_on_transport_error(): void
    {
        $checker = new RevocationChecker(
            $this->clientReturning([
                new \GuzzleHttp\Exception\ConnectException('boom', new \GuzzleHttp\Psr7\Request('GET', 'https://fc.test')),
            ]),
            $this->cache(),
            'https://fc.test',
        );

        $this->assertFalse($checker->isStale('prn_1', 'token', 0));
    }
}
