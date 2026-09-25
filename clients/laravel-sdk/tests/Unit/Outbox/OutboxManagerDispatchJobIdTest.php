<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Outbox;

use FlowCatalyst\Outbox\Contracts\OutboxDriver;
use FlowCatalyst\Outbox\DTOs\CreateDispatchJobDto;
use FlowCatalyst\Outbox\DTOs\CreateEventDto;
use FlowCatalyst\Outbox\OutboxManager;
use PHPUnit\Framework\TestCase;

/**
 * The platform honours a supplied dispatch-job id (1-13 of [A-Za-z0-9_-]), so
 * the outbox row's own id travels in the payload: a resend after a lost answer
 * is recognised instead of creating a second job.
 */
final class OutboxManagerDispatchJobIdTest extends TestCase
{
    private function capturingDriver(): OutboxDriver
    {
        return new class implements OutboxDriver {
            /** @var array<int, array<string, mixed>> */
            public array $messages = [];

            public function insert(array $message): void
            {
                $this->messages[] = $message;
            }

            public function insertBatch(array $messages): void
            {
                foreach ($messages as $message) {
                    $this->messages[] = $message;
                }
            }
        };
    }

    private function job(): CreateDispatchJobDto
    {
        return CreateDispatchJobDto::create(
            source: 'orders',
            code: 'orders:fulfilment:order:shipped',
            targetUrl: 'https://example.test/hook',
            payload: '{}',
            dispatchPoolId: 'dpl_1',
        );
    }

    public function test_a_dispatch_job_payload_carries_its_outbox_row_id(): void
    {
        $driver = $this->capturingDriver();
        $outbox = new OutboxManager($driver, 'clt_1');

        $one = $outbox->createDispatchJob($this->job());
        $many = $outbox->createDispatchJobs([$this->job(), $this->job()]);

        $ids = [$one, ...$many];
        $this->assertCount(3, $driver->messages);
        foreach ($driver->messages as $i => $message) {
            $this->assertSame($ids[$i], $message['id']);
            $payload = json_decode($message['payload'], true);
            $this->assertSame($ids[$i], $payload['id']);
            $this->assertMatchesRegularExpression('/^[A-Za-z0-9_-]{1,13}$/', $payload['id']);
            $this->assertSame('orders:fulfilment:order:shipped', $payload['code']);
            $this->assertSame(strlen($message['payload']), $message['payload_size']);
        }
    }

    public function test_events_are_unchanged(): void
    {
        $driver = $this->capturingDriver();
        $outbox = new OutboxManager($driver, 'clt_1');

        $outbox->createEvent(CreateEventDto::create('shop:orders:order:placed', ['a' => 1]));

        $payload = json_decode($driver->messages[0]['payload'], true);
        $this->assertArrayNotHasKey('id', $payload);
    }
}
