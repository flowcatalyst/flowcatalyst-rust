<?php

declare(strict_types=1);

namespace FlowCatalyst\ScheduledJobs;

/**
 * Contract for zero-HTTP scheduled-job handling: put #[AsScheduledJob] on a
 * class implementing this interface and the SDK wires everything else — the
 * definition sync registers the job (with the auto-mounted process route as
 * its target URL), and the auto-registered route resolves the class from the
 * container and invokes handle() when the platform fires.
 *
 * The interface is optional: any scanned #[AsScheduledJob] class with a
 * public `handle(array $envelope, callable $log): mixed` method is wired the
 * same way. Implementing it just makes the contract explicit and type-checked.
 *
 * A manual `$runner->handler($code, ...)` registration for the same job code
 * always wins over the scanned class.
 */
interface HandlesScheduledJob
{
    /**
     * Handle one firing. The envelope is the platform's delivery payload
     * (jobId, jobCode, instanceId, firedAt, triggerKind, payload, ...); $log
     * appends a structured log entry to the firing's instance. Return any
     * JSON-serialisable value — used as the completion result when the job
     * tracks completion. Throw to mark the firing FAILED.
     *
     * @param array<string, mixed> $envelope
     * @param callable(string, string=, mixed=): void $log
     */
    public function handle(array $envelope, callable $log): mixed;
}
