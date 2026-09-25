<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Outbox;

use FlowCatalyst\Outbox\DTOs\CreateDispatchJobDto;
use PHPUnit\Framework\TestCase;

final class CreateDispatchJobDtoTest extends TestCase
{
    public function test_queue_is_omitted_when_unset_but_carried_when_set(): void
    {
        $base = CreateDispatchJobDto::create(
            source: 'svc',
            code: 'app:sub:agg:act',
            targetUrl: 'https://example.com/hook',
            payload: '{"k":1}',
            dispatchPoolId: 'pool-1',
        );

        $this->assertArrayNotHasKey(
            'queue',
            $base->toPayload(),
            'an unset queue must be omitted, never defaulted',
        );

        $withQueue = $base->withQueue('HIGH_PRIORITY');
        $this->assertSame('HIGH_PRIORITY', $withQueue->toPayload()['queue']);
    }

    /**
     * Every wither hand-copies all fields into a new instance, so one that
     * forgets `queue` drops it the moment it is chained after withQueue() —
     * invisible to a test that only calls withQueue() last. Chains through a
     * later wither instead.
     */
    public function test_queue_survives_a_later_wither(): void
    {
        $job = CreateDispatchJobDto::create(
            source: 'svc',
            code: 'app:sub:agg:act',
            targetUrl: 'https://example.com/hook',
            payload: '{"k":1}',
            dispatchPoolId: 'pool-1',
        )->withQueue('HIGH_PRIORITY')->withMessageGroup('grp-1');

        $payload = $job->toPayload();
        $this->assertSame('HIGH_PRIORITY', $payload['queue'], 'a later wither must not drop queue');
        $this->assertSame('grp-1', $payload['messageGroup']);
    }
}
