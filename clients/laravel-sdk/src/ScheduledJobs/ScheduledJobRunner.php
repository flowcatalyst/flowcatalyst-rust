<?php

declare(strict_types=1);

namespace FlowCatalyst\ScheduledJobs;

use FlowCatalyst\Client\Resources\ScheduledJobs;
use Throwable;

/**
 * ScheduledJobRunner — handler registry + envelope dispatch + lock + optional
 * completion callback.
 *
 * Zero-config inside Laravel: the service provider binds a runner singleton
 * (cache-lock overlap protection included), mounts it at
 * DEFAULT_PROCESS_PATH, and the definition sync points each job's targetUrl
 * there — apps only register handlers:
 *
 *   app(ScheduledJobRunner::class)->handler('nightly-report', fn ($env, $log) => ...);
 *
 * To mount manually instead (extra middleware, custom path), disable
 * `flowcatalyst.scheduled_jobs.register_route` and route to
 * `processWithResponse()` yourself:
 *
 *   Route::post('/my/path', function (Request $req) use ($runner) {
 *       [$status, $body] = $runner->processWithResponse($req->json()->all());
 *       return response()->json($body, $status);
 *   });
 *
 * Returns 202 immediately on a recognised envelope and runs the handler
 * synchronously before returning — Laravel doesn't have native fire-and-
 * forget HTTP semantics, so handlers should keep work brief or queue it
 * onto Laravel's job queue themselves. The platform's HTTP timeout
 * (default 10s) is the deadline.
 */
final class ScheduledJobRunner
{
    /**
     * Conventional process path. The service provider registers a POST route
     * here (config `flowcatalyst.scheduled_jobs.register_route`), and the
     * definition sync defaults each job's targetUrl to APP_URL + this path
     * when the attribute doesn't set one.
     */
    public const DEFAULT_PROCESS_PATH = '/api/_fc/scheduled-jobs/process';

    /** @var array<string, callable> */
    private array $handlers = [];

    private LockProvider $lockProvider;
    /** @var (callable(array): string)|null */
    private $lockKeyResolver;
    /** null = follow the envelope's `concurrent` attribute (lock unless concurrent). */
    private ?bool $enforceLock;
    private int $lockTtlSeconds;
    /** @var (callable(Throwable, array): void)|null */
    private $onError;

    /**
     * @param array{
     *   lockProvider?: LockProvider,
     *   lockKey?: callable(array): string,
     *   enforceLock?: bool,
     *   lockTtlSeconds?: int,
     *   onError?: callable(Throwable, array): void,
     * } $options
     */
    public function __construct(
        private readonly ScheduledJobs $resource,
        array $options = [],
    ) {
        $this->lockProvider = $options['lockProvider'] ?? self::defaultLockProvider();
        $this->lockKeyResolver = $options['lockKey'] ?? null;
        // When the option is omitted, locking follows the job definition: the
        // platform echoes the `concurrent` attribute in the envelope, so
        // concurrent: false jobs (the AsScheduledJob default) get overlap
        // protection with zero configuration. An explicit true/false overrides.
        $this->enforceLock = $options['enforceLock'] ?? null;
        $this->lockTtlSeconds = $options['lockTtlSeconds'] ?? 600;
        $this->onError = $options['onError'] ?? null;
    }

    /**
     * Zero-config overlap protection: inside a booted Laravel app, use the
     * app's default cache store (Laravel's atomic cache locks survive
     * multiple replicas as long as they share the backend). Outside Laravel
     * — or before the container is available — fall back to no-op, same as
     * the old default.
     */
    private static function defaultLockProvider(): LockProvider
    {
        if (class_exists(\Illuminate\Container\Container::class)) {
            try {
                $app = \Illuminate\Container\Container::getInstance();
                if ($app !== null && $app->bound('cache.store')) {
                    /** @var \Illuminate\Contracts\Cache\Repository $store */
                    $store = $app->make('cache.store');
                    return new CacheLockProvider($store);
                }
            } catch (Throwable) {
                // fall through to no-op
            }
        }
        return new NoOpLockProvider();
    }

    /**
     * Whether this firing must hold the overlap lock: the explicit runner
     * option wins; otherwise follow the envelope's `concurrent` attribute
     * (absent on pre-echo platforms ⇒ treat as non-concurrent ⇒ lock).
     *
     * @param array<string, mixed> $envelope
     */
    private function shouldLock(array $envelope): bool
    {
        if ($this->enforceLock !== null) {
            return $this->enforceLock;
        }
        return !(bool) ($envelope['concurrent'] ?? false);
    }

    /**
     * Register a handler keyed by the job's `code`. The handler receives the
     * envelope as an array and a logger callable, and may return any
     * JSON-serialisable value (used as `completion_result` when the job
     * tracks completion).
     *
     * @param callable(array, callable(string, string=, mixed=): void): mixed $handler
     */
    public function handler(string $code, callable $handler): self
    {
        $this->handlers[$code] = $handler;
        return $this;
    }

    /** @return string[] */
    public function registeredCodes(): array
    {
        return array_keys($this->handlers);
    }

    public function hasHandler(string $code): bool
    {
        return isset($this->handlers[$code]);
    }

    /**
     * Wire handlers from scanned #[AsScheduledJob] definition rows: every row
     * whose `_class` has a public `handle(array $envelope, callable $log)`
     * method (see {@see HandlesScheduledJob}) gets a handler that lazily
     * resolves the class through $make and invokes it. Codes that already
     * have a handler are left alone — manual registration always wins.
     *
     * @param array<array<string, mixed>> $rows Scanned definition rows
     * @param callable(string): object $make Class-string → instance factory
     * @return int Number of handlers registered
     */
    public static function registerScannedHandlers(self $runner, array $rows, callable $make): int
    {
        $registered = 0;
        foreach ($rows as $row) {
            $code = (string) ($row['code'] ?? '');
            $class = (string) ($row['_class'] ?? '');
            if ($code === '' || $class === '' || $runner->hasHandler($code)) {
                continue;
            }
            if (!class_exists($class) || !method_exists($class, 'handle')) {
                continue;
            }
            $runner->handler(
                $code,
                static fn (array $envelope, callable $log): mixed => $make($class)->handle($envelope, $log),
            );
            $registered++;
        }
        return $registered;
    }

    /**
     * Process an inbound platform → SDK firing.
     *
     * @param array<string, mixed> $envelope
     * @return array{0: int, 1: array<string, mixed>} [status, bodyJson]
     */
    public function processWithResponse(array $envelope): array
    {
        $error = $this->validateEnvelope($envelope);
        if ($error !== null) {
            return [400, ['error' => $error]];
        }

        $code = (string) $envelope['jobCode'];
        if (!isset($this->handlers[$code])) {
            return [404, ['error' => "No handler registered for code '{$code}'"]];
        }

        $this->runOne($envelope, $this->handlers[$code]);
        return [202, ['ok' => true]];
    }

    /**
     * @param array<string, mixed> $envelope
     */
    private function runOne(array $envelope, callable $handler): void
    {
        $release = null;
        try {
            if ($this->shouldLock($envelope)) {
                $key = $this->lockKeyResolver !== null
                    ? ($this->lockKeyResolver)($envelope)
                    : 'scheduled-job:' . $envelope['jobCode'];
                $release = $this->lockProvider->acquire($key, $this->lockTtlSeconds);
                if ($release === null) {
                    if ((bool) ($envelope['tracksCompletion'] ?? false)) {
                        $this->safeComplete($envelope, 'FAILURE', [
                            'skipped' => true,
                            'reason' => 'lock-held',
                        ]);
                    }
                    return;
                }
            }

            $instanceId = (string) $envelope['instanceId'];
            $logger = function (string $message, string $level = 'INFO', mixed $metadata = null) use ($instanceId): void {
                try {
                    $this->resource->logForInstance($instanceId, $message, $level, $metadata);
                } catch (Throwable $e) {
                    if ($this->onError) ($this->onError)($e, ['instanceId' => $instanceId]);
                }
            };

            $succeeded = true;
            $result = null;
            $thrown = null;
            try {
                $result = $handler($envelope, $logger);
            } catch (Throwable $e) {
                $succeeded = false;
                $thrown = $e;
            }

            if ((bool) ($envelope['tracksCompletion'] ?? false)) {
                $this->safeComplete(
                    $envelope,
                    $succeeded ? 'SUCCESS' : 'FAILURE',
                    $succeeded
                        ? $this->sanitiseResult($result)
                        : ['error' => $thrown?->getMessage() ?? 'Unknown error'],
                );
            }

            if (!$succeeded && $this->onError) {
                ($this->onError)($thrown, $envelope);
            }
        } finally {
            if ($release !== null) {
                try { $release(); } catch (Throwable $e) {
                    if ($this->onError) ($this->onError)($e, $envelope);
                }
            }
        }
    }

    /**
     * @param array<string, mixed> $envelope
     */
    private function safeComplete(array $envelope, string $status, mixed $result): void
    {
        try {
            $this->resource->completeInstance(
                (string) $envelope['instanceId'],
                $status,
                $result,
            );
        } catch (Throwable $e) {
            if ($this->onError) ($this->onError)($e, $envelope);
        }
    }

    /**
     * @param array<string, mixed> $envelope
     */
    private function validateEnvelope(array $envelope): ?string
    {
        foreach (['jobId', 'jobCode', 'instanceId', 'firedAt', 'triggerKind'] as $required) {
            if (!isset($envelope[$required]) || !is_string($envelope[$required])) {
                return "Envelope missing string field '{$required}'";
            }
        }
        if (!isset($envelope['tracksCompletion']) || !is_bool($envelope['tracksCompletion'])) {
            return "Envelope missing boolean field 'tracksCompletion'";
        }
        $trigger = (string) $envelope['triggerKind'];
        if ($trigger !== 'CRON' && $trigger !== 'MANUAL') {
            return "Invalid triggerKind '{$trigger}'";
        }
        return null;
    }

    private function sanitiseResult(mixed $v): mixed
    {
        // Cap at ~10KB so completion_result doesn't bloat the row.
        try {
            $json = json_encode($v, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE);
            if ($json === false) return ['unserialisable' => true];
            if (strlen($json) > 10_000) {
                return ['truncated' => true, 'preview' => substr($json, 0, 10_000)];
            }
            return $v;
        } catch (Throwable) {
            return ['unserialisable' => true];
        }
    }
}
