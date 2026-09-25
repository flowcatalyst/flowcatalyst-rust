<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Outbox;

use FlowCatalyst\Outbox\DTOs\CreateDispatchJobDto;
use FlowCatalyst\Outbox\DTOs\CreateEventDto;
use InvalidArgumentException;
use PHPUnit\Framework\TestCase;

/**
 * The SDK must refuse to emit unqualified codes: the platform facets on the
 * four application:subdomain:aggregate:action segments and resolves delivery
 * signing credentials from the application segment, so a bare one-word code
 * renders as `name:::`, facets under the wrong application, and delivers
 * unsigned.
 */
final class QualifiedCodeTest extends TestCase
{
    public function test_accepts_fully_qualified_codes(): void
    {
        $job = new CreateDispatchJobDto(
            source: 'fulfil-go',
            code: 'fulfil-go:fulfilment:part:create-pick',
            targetUrl: 'https://example.com/hook',
            payload: '{}',
            dispatchPoolId: 'pool-1',
        );
        $this->assertSame('fulfil-go:fulfilment:part:create-pick', $job->code);

        $event = CreateEventDto::create('fulfil-go:fulfilment:part:picked', ['ok' => true]);
        $this->assertSame('fulfil-go:fulfilment:part:picked', $event->type);
    }

    public function test_rejects_unqualified_codes(): void
    {
        foreach (['create-pick', 'a:b:c', 'a:b:c:d:e', 'a::c:d'] as $bad) {
            try {
                new CreateDispatchJobDto(
                    source: 's',
                    code: $bad,
                    targetUrl: 'https://x',
                    payload: '{}',
                    dispatchPoolId: 'p',
                );
                $this->fail("dispatch code '{$bad}' must be rejected");
            } catch (InvalidArgumentException $e) {
                $this->assertStringContainsString('fully qualified', $e->getMessage());
            }

            try {
                CreateEventDto::create($bad, []);
                $this->fail("event type '{$bad}' must be rejected");
            } catch (InvalidArgumentException $e) {
                $this->assertStringContainsString('fully qualified', $e->getMessage());
            }
        }
    }
}
