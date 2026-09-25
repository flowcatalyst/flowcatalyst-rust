<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\ScheduledJobs;

use FlowCatalyst\Client\Resources\ScheduledJobs;
use FlowCatalyst\ScheduledJobs\LockProvider;
use FlowCatalyst\ScheduledJobs\ScheduledJobRunner;
use PHPUnit\Framework\TestCase;

/**
 * Overlap protection must be zero-config and follow the job definition:
 * the platform echoes the `concurrent` attribute in the firing envelope, and
 * the runner locks exactly when the definition asks for it (concurrent=false,
 * the AsScheduledJob default). Explicit enforceLock still overrides, and
 * envelopes from older platforms (no `concurrent` key) are treated as
 * non-concurrent — the safe direction.
 */
final class ScheduledJobRunnerLockTest extends TestCase
{
    /** @var string[] */
    private array $acquired = [];

    private function recordingLockProvider(): LockProvider
    {
        $this->acquired = [];
        $acquired = &$this->acquired;
        return new class($acquired) implements LockProvider {
            /** @param string[] $acquired */
            public function __construct(private array &$acquired) {}

            public function acquire(string $key, int $ttlSeconds): ?callable
            {
                $this->acquired[] = $key;
                return static function (): void {};
            }
        };
    }

    /**
     * @param array<string, mixed> $overrides
     * @return array<string, mixed>
     */
    private function envelope(array $overrides = []): array
    {
        return array_merge([
            'jobId' => 'sjb_1',
            'jobCode' => 'nightly-report',
            'instanceId' => 'sji_1',
            'firedAt' => '2026-08-07T00:00:00Z',
            'triggerKind' => 'CRON',
            'tracksCompletion' => false,
        ], $overrides);
    }

    private function runner(LockProvider $lock, array $options = []): ScheduledJobRunner
    {
        $resource = $this->createMock(ScheduledJobs::class);
        $runner = new ScheduledJobRunner($resource, array_merge(['lockProvider' => $lock], $options));
        $runner->handler('nightly-report', static fn (array $env, callable $log): bool => true);
        return $runner;
    }

    public function test_non_concurrent_envelope_locks_by_default(): void
    {
        $runner = $this->runner($this->recordingLockProvider());
        [$status] = $runner->processWithResponse($this->envelope(['concurrent' => false]));
        $this->assertSame(202, $status);
        $this->assertSame(['scheduled-job:nightly-report'], $this->acquired);
    }

    public function test_missing_concurrent_key_locks_by_default(): void
    {
        $runner = $this->runner($this->recordingLockProvider());
        $runner->processWithResponse($this->envelope());
        $this->assertSame(['scheduled-job:nightly-report'], $this->acquired);
    }

    public function test_concurrent_envelope_skips_lock(): void
    {
        $runner = $this->runner($this->recordingLockProvider());
        $runner->processWithResponse($this->envelope(['concurrent' => true]));
        $this->assertSame([], $this->acquired);
    }

    public function test_explicit_enforce_lock_overrides_envelope(): void
    {
        $runner = $this->runner($this->recordingLockProvider(), ['enforceLock' => true]);
        $runner->processWithResponse($this->envelope(['concurrent' => true]));
        $this->assertSame(['scheduled-job:nightly-report'], $this->acquired);

        $runner = $this->runner($this->recordingLockProvider(), ['enforceLock' => false]);
        $runner->processWithResponse($this->envelope(['concurrent' => false]));
        $this->assertSame([], $this->acquired);
    }
}
