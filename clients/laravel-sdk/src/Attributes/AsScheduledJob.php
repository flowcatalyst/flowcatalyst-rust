<?php

declare(strict_types=1);

namespace FlowCatalyst\Attributes;

use Attribute;

/**
 * Marks a class as a scheduled-job definition for FlowCatalyst.
 *
 * Usage:
 * ```php
 * #[AsScheduledJob(
 *     code: 'nightly-report',
 *     name: 'Nightly Report',
 *     crons: ['0 0 2 * * *'],
 *     timezone: 'Europe/London',
 *     description: 'Aggregate prior-day metrics and email to ops',
 *     concurrent: false,
 *     tracksCompletion: true,
 *     timeoutSeconds: 600,
 * )]
 * class NightlyReportJob {}
 * ```
 *
 * The code is stored on the platform EXACTLY as declared here — no
 * application prefix is added at sync. Declare the full code you want (e.g.
 * "orders:nightly-report"); the firing envelope's `jobCode` carries the same
 * string, and auto-handler wiring keys on it verbatim.
 *
 * `crons` requires 6-field, seconds-first expressions (`sec min hour dom
 * month dow`) — a standard 5-field cron passes validation but never fires.
 * `timezone` defaults to UTC server-side when omitted. `concurrent` controls
 * whether the platform fires a new tick while a previous invocation is
 * still running (default false). `tracksCompletion` flips the platform
 * from "webhook delivery is the success signal" to "consumer POSTs to
 * /complete when done".
 *
 * `clientId` scopes the job to a client/tenant rather than the platform —
 * omit it only for platform-wide jobs (anchor-only). This is a different
 * axis from the application code: the application groups the job's code
 * namespace, `clientId` controls who owns and can manage the job.
 */
#[Attribute(Attribute::TARGET_CLASS)]
final class AsScheduledJob
{
    /**
     * @param string $code Short code (no `<app>:` prefix — added at sync time)
     * @param string $name Human-friendly label
     * @param string[] $crons 6-field, seconds-first cron expressions
     * @param string|null $description Short summary
     * @param string|null $timezone IANA tz name (default UTC server-side)
     * @param array<string, mixed>|null $payload JSON payload sent to the consumer
     * @param bool $concurrent Allow overlapping invocations (default false)
     * @param bool $tracksCompletion Consumer reports via /complete (default false)
     * @param int|null $timeoutSeconds Per-invocation timeout
     * @param int|null $deliveryMaxAttempts Webhook delivery retries (default 3)
     * @param string|null $targetUrl Override the callback URL (default: APP_URL + the SDK's /api/_fc/scheduled-jobs/process route)
     * @param string|null $clientId Client/tenant that owns this job. Null = platform-scoped (anchor only).
     * @param string|null $application Application code this job belongs to.
     *        Overrides the global `application_code` / `application_map` during
     *        sync — set it when one codebase defines scheduled jobs for more
     *        than one application. Null = resolve from the namespace map /
     *        default.
     */
    public function __construct(
        public readonly string $code,
        public readonly string $name,
        public readonly array $crons,
        public readonly ?string $description = null,
        public readonly ?string $timezone = null,
        public readonly ?array $payload = null,
        public readonly bool $concurrent = false,
        public readonly bool $tracksCompletion = false,
        public readonly ?int $timeoutSeconds = null,
        public readonly ?int $deliveryMaxAttempts = null,
        public readonly ?string $targetUrl = null,
        public readonly ?string $clientId = null,
        public readonly ?string $application = null,
    ) {}

    /**
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        $data = [
            'code' => $this->code,
            'name' => $this->name,
            'crons' => $this->crons,
            'concurrent' => $this->concurrent,
            'tracksCompletion' => $this->tracksCompletion,
        ];
        if ($this->description !== null) {
            $data['description'] = $this->description;
        }
        if ($this->timezone !== null) {
            $data['timezone'] = $this->timezone;
        }
        if ($this->payload !== null) {
            $data['payload'] = $this->payload;
        }
        if ($this->timeoutSeconds !== null) {
            $data['timeoutSeconds'] = $this->timeoutSeconds;
        }
        if ($this->deliveryMaxAttempts !== null) {
            $data['deliveryMaxAttempts'] = $this->deliveryMaxAttempts;
        }
        if ($this->targetUrl !== null) {
            $data['targetUrl'] = $this->targetUrl;
        }
        if ($this->clientId !== null) {
            $data['clientId'] = $this->clientId;
        }
        return $data;
    }
}
