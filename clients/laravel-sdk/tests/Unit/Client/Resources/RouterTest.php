<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Client\Resources;

use FlowCatalyst\Client\Auth\UserTokenProvider;
use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\Client\Resources\Router;
use GuzzleHttp\Client;
use GuzzleHttp\Handler\MockHandler;
use GuzzleHttp\HandlerStack;
use GuzzleHttp\Middleware;
use GuzzleHttp\Psr7\Response;
use PHPUnit\Framework\TestCase;

/**
 * docs/spec/router-api-auth.md rule 8: the router verifies the same platform
 * bearer token, so both in-flight checks must carry it.
 */
final class RouterTest extends TestCase
{
    public function testTheInFlightChecksSendThePlatformToken(): void
    {
        $history = new \ArrayObject();
        $stack = HandlerStack::create(new MockHandler([
            new Response(200, [], '{"messageId":"m1","inPipeline":false}'),
            new Response(200, [], '{"m1":true}'),
        ]));
        $stack->push(Middleware::history($history));
        $routerHttp = new Client(['handler' => $stack, 'http_errors' => false, 'base_uri' => 'https://router.test']);

        $client = new FlowCatalystClient(
            tokenProvider: new UserTokenProvider('tok-1'),
            baseUrl: 'https://fc.test',
            routerBaseUrl: 'https://router.test',
        );
        $router = new Router($client, $routerHttp);

        $this->assertFalse($router->inPipeline('m1')['inPipeline']);
        $this->assertSame(['m1' => true], $router->inPipelineBatch(['m1']));

        $this->assertCount(2, $history);
        foreach ($history as $transaction) {
            $this->assertSame('Bearer tok-1', $transaction['request']->getHeaderLine('Authorization'));
        }
    }
}
