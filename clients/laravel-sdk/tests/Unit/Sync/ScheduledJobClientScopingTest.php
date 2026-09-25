<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Sync;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\Client\Resources\ScheduledJobs;
use FlowCatalyst\Sync\DefinitionSynchronizer;
use FlowCatalyst\Sync\SyncDefinitionSet;
use PHPUnit\Framework\TestCase;

/**
 * The scheduled-jobs sync endpoint accepts one `clientId` per HTTP call, not
 * per job. `DefinitionSynchronizer::syncScheduledJobs()` must group jobs by
 * their resolved clientId and issue one `sync()` call per distinct group —
 * previously it never forwarded clientId at all, so every job silently
 * landed platform-scoped regardless of what the caller set.
 */
final class ScheduledJobClientScopingTest extends TestCase
{
    public function test_single_shared_client_id_produces_one_sync_call(): void
    {
        $scheduledJobs = $this->createMock(ScheduledJobs::class);
        $scheduledJobs->expects($this->once())
            ->method('sync')
            ->with(
                $this->equalTo('orders'),
                $this->callback(fn(array $jobs) => count($jobs) === 1),
                $this->equalTo('clt_abc'),
                $this->equalTo(false),
            )
            ->willReturn(['created' => ['sj_1'], 'updated' => [], 'archived' => []]);

        $client = $this->createMock(FlowCatalystClient::class);
        $client->method('scheduledJobs')->willReturn($scheduledJobs);

        $synchronizer = new DefinitionSynchronizer($client);
        $set = SyncDefinitionSet::forApplication('orders')->withScheduledJobs([
            ['code' => 'nightly-report', 'name' => 'Nightly Report', 'crons' => ['0 0 2 * * *'], 'clientId' => 'clt_abc'],
        ]);

        $result = $synchronizer->sync($set);

        $this->assertSame(1, $result->scheduledJobs['created']);
        $this->assertArrayNotHasKey('error', $result->scheduledJobs);
    }

    public function test_mixed_client_ids_produce_one_sync_call_per_group_and_sum_results(): void
    {
        $scheduledJobs = $this->createMock(ScheduledJobs::class);
        $scheduledJobs->expects($this->exactly(2))
            ->method('sync')
            ->willReturnCallback(function (string $appCode, array $jobs, ?string $clientId, bool $archiveUnlisted) {
                $this->assertSame('orders', $appCode);
                $this->assertCount(1, $jobs);
                $this->assertContains($clientId, ['clt_one', 'clt_two']);
                return ['created' => ['sj_' . $clientId], 'updated' => [], 'archived' => []];
            });

        $client = $this->createMock(FlowCatalystClient::class);
        $client->method('scheduledJobs')->willReturn($scheduledJobs);

        $synchronizer = new DefinitionSynchronizer($client);
        $set = SyncDefinitionSet::forApplication('orders')->withScheduledJobs([
            ['code' => 'job-a', 'name' => 'Job A', 'crons' => ['0 0 2 * * *'], 'clientId' => 'clt_one'],
            ['code' => 'job-b', 'name' => 'Job B', 'crons' => ['0 0 3 * * *'], 'clientId' => 'clt_two'],
        ]);

        $result = $synchronizer->sync($set);

        $this->assertSame(2, $result->scheduledJobs['created']);
    }

    public function test_no_client_id_stays_platform_scoped(): void
    {
        $scheduledJobs = $this->createMock(ScheduledJobs::class);
        $scheduledJobs->expects($this->once())
            ->method('sync')
            ->with($this->anything(), $this->anything(), null, $this->anything())
            ->willReturn(['created' => ['sj_1'], 'updated' => [], 'archived' => []]);

        $client = $this->createMock(FlowCatalystClient::class);
        $client->method('scheduledJobs')->willReturn($scheduledJobs);

        $synchronizer = new DefinitionSynchronizer($client);
        $set = SyncDefinitionSet::forApplication('orders')->withScheduledJobs([
            ['code' => 'nightly-report', 'name' => 'Nightly Report', 'crons' => ['0 0 2 * * *']],
        ]);

        $synchronizer->sync($set);
    }
}
