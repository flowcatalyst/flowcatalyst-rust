<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\ScheduledJobs;

use FlowCatalyst\Client\Resources\ScheduledJobs;
use FlowCatalyst\ScheduledJobs\HandlesScheduledJob;
use FlowCatalyst\ScheduledJobs\NoOpLockProvider;
use FlowCatalyst\ScheduledJobs\ScheduledJobRunner;
use PHPUnit\Framework\TestCase;

final class AutoWiredJob implements HandlesScheduledJob
{
    /** @var array<string, mixed>|null */
    public static ?array $seenEnvelope = null;

    public function handle(array $envelope, callable $log): mixed
    {
        self::$seenEnvelope = $envelope;
        return ['ok' => true];
    }
}

final class NotAHandler
{
}

/**
 * Zero-HTTP scheduled jobs: scanned #[AsScheduledJob] rows whose `_class`
 * exposes handle() are wired automatically, keyed by the declared code (the
 * same code the firing envelope carries — the sync stores it verbatim).
 * Manual registrations always win; rows without a usable class are skipped.
 */
final class ScheduledJobRunnerAutoHandlersTest extends TestCase
{
    private function runner(): ScheduledJobRunner
    {
        return new ScheduledJobRunner(
            $this->createMock(ScheduledJobs::class),
            ['lockProvider' => new NoOpLockProvider()],
        );
    }

    public function test_registers_scanned_class_and_dispatches_to_it(): void
    {
        AutoWiredJob::$seenEnvelope = null;
        $runner = $this->runner();

        $count = ScheduledJobRunner::registerScannedHandlers($runner, [
            ['code' => 'auto-wired', '_class' => AutoWiredJob::class],
            ['code' => 'no-class'],
            ['code' => 'not-a-handler', '_class' => NotAHandler::class],
            ['code' => 'missing-class', '_class' => 'App\\DoesNotExist'],
        ], static fn (string $class): object => new $class());

        $this->assertSame(1, $count);
        $this->assertTrue($runner->hasHandler('auto-wired'));
        $this->assertFalse($runner->hasHandler('not-a-handler'));

        [$status] = $runner->processWithResponse([
            'jobId' => 'sjb_1',
            'jobCode' => 'auto-wired',
            'instanceId' => 'sji_1',
            'firedAt' => '2026-08-07T00:00:00Z',
            'triggerKind' => 'CRON',
            'tracksCompletion' => false,
            'concurrent' => true,
        ]);
        $this->assertSame(202, $status);
        $this->assertNotNull(AutoWiredJob::$seenEnvelope);
        $this->assertSame('sji_1', AutoWiredJob::$seenEnvelope['instanceId']);
    }

    public function test_manual_registration_wins_over_scanned_class(): void
    {
        $runner = $this->runner();
        $manualCalled = false;
        $runner->handler('auto-wired', function () use (&$manualCalled) {
            $manualCalled = true;
            return null;
        });

        $count = ScheduledJobRunner::registerScannedHandlers($runner, [
            ['code' => 'auto-wired', '_class' => AutoWiredJob::class],
        ], static fn (string $class): object => new $class());
        $this->assertSame(0, $count, 'existing handler must not be overridden');

        AutoWiredJob::$seenEnvelope = null;
        $runner->processWithResponse([
            'jobId' => 'sjb_1',
            'jobCode' => 'auto-wired',
            'instanceId' => 'sji_2',
            'firedAt' => '2026-08-07T00:00:00Z',
            'triggerKind' => 'MANUAL',
            'tracksCompletion' => false,
            'concurrent' => true,
        ]);
        $this->assertTrue($manualCalled);
        $this->assertNull(AutoWiredJob::$seenEnvelope, 'scanned class must not run');
    }
}
