<?php

declare(strict_types=1);

namespace FlowCatalyst\Sync;

/**
 * Represents a scheduled-job definition for syncing to FlowCatalyst.
 *
 * `code` is the platform-side identifier; convention is
 * `{application}:{job-name}` but not enforced. `crons` requires 6-field,
 * seconds-first expressions (`sec min hour dom month dow`) — a standard
 * 5-field cron passes validation but never fires; the platform evaluates
 * them in `timezone` (defaults to UTC server-side when null).
 *
 * `concurrent: true` lets the platform fire while a previous invocation
 * is still running. `tracksCompletion: true` enables per-instance
 * status tracking via the `POST /api/scheduled-jobs/instances/{id}/complete`
 * callback rather than treating webhook delivery as the success signal.
 *
 * `clientId` scopes the job to a client/tenant rather than the platform —
 * leave it null only for platform-wide jobs (anchor-only).
 */
final class ScheduledJobDefinition
{
    /**
     * @param string $code Full platform code (e.g. "orders:nightly-report")
     * @param string $name Human-readable label
     * @param string[] $crons 6-field, seconds-first cron expressions
     * @param string|null $description Optional summary
     * @param string|null $timezone IANA tz name; defaults to UTC server-side
     * @param array<string, mixed>|null $payload JSON payload sent to the consumer
     * @param bool $concurrent Allow overlapping invocations (default false)
     * @param bool $tracksCompletion Consumer reports back via /complete (default false)
     * @param int|null $timeoutSeconds Per-invocation timeout
     * @param int|null $deliveryMaxAttempts Webhook delivery retries (default 3)
     * @param string|null $targetUrl Override the application's default callback URL
     * @param string|null $clientId Client/tenant that owns this job. Null = platform-scoped (anchor only).
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
    ) {}

    public function getCode(): string
    {
        return $this->code;
    }

    /**
     * Fluent factory.
     *
     * @param string[] $crons
     */
    public static function make(
        string $code,
        string $name,
        array $crons,
    ): self {
        return new self(code: $code, name: $name, crons: $crons);
    }

    public function withDescription(string $description): self
    {
        return $this->copy(description: $description);
    }

    public function withTimezone(string $timezone): self
    {
        return $this->copy(timezone: $timezone);
    }

    /**
     * @param array<string, mixed> $payload
     */
    public function withPayload(array $payload): self
    {
        return $this->copy(payload: $payload);
    }

    public function withConcurrent(bool $concurrent): self
    {
        return $this->copy(concurrent: $concurrent);
    }

    public function withTracksCompletion(bool $tracksCompletion): self
    {
        return $this->copy(tracksCompletion: $tracksCompletion);
    }

    public function withTimeoutSeconds(int $timeoutSeconds): self
    {
        return $this->copy(timeoutSeconds: $timeoutSeconds);
    }

    public function withDeliveryMaxAttempts(int $deliveryMaxAttempts): self
    {
        return $this->copy(deliveryMaxAttempts: $deliveryMaxAttempts);
    }

    public function withTargetUrl(string $targetUrl): self
    {
        return $this->copy(targetUrl: $targetUrl);
    }

    public function withClientId(string $clientId): self
    {
        return $this->copy(clientId: $clientId);
    }

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

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        return new self(
            code: $data['code'],
            name: $data['name'],
            crons: $data['crons'] ?? [],
            description: $data['description'] ?? null,
            timezone: $data['timezone'] ?? null,
            payload: $data['payload'] ?? null,
            concurrent: $data['concurrent'] ?? false,
            tracksCompletion: $data['tracksCompletion'] ?? false,
            timeoutSeconds: $data['timeoutSeconds'] ?? null,
            deliveryMaxAttempts: $data['deliveryMaxAttempts'] ?? null,
            targetUrl: $data['targetUrl'] ?? null,
            clientId: $data['clientId'] ?? null,
        );
    }

    /**
     * @param array<string, mixed>|null $payload
     * @param string[]|null $crons
     */
    private function copy(
        ?string $description = null,
        ?string $timezone = null,
        ?array $payload = null,
        ?bool $concurrent = null,
        ?bool $tracksCompletion = null,
        ?int $timeoutSeconds = null,
        ?int $deliveryMaxAttempts = null,
        ?string $targetUrl = null,
        ?string $clientId = null,
    ): self {
        return new self(
            code: $this->code,
            name: $this->name,
            crons: $this->crons,
            description: $description ?? $this->description,
            timezone: $timezone ?? $this->timezone,
            payload: $payload ?? $this->payload,
            concurrent: $concurrent ?? $this->concurrent,
            tracksCompletion: $tracksCompletion ?? $this->tracksCompletion,
            timeoutSeconds: $timeoutSeconds ?? $this->timeoutSeconds,
            deliveryMaxAttempts: $deliveryMaxAttempts ?? $this->deliveryMaxAttempts,
            targetUrl: $targetUrl ?? $this->targetUrl,
            clientId: $clientId ?? $this->clientId,
        );
    }
}
