<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

final class CreateScheduledJobRequest
{
    /**
     * @param string[] $crons
     */
    public function __construct(
        public readonly string $code,
        public readonly string $name,
        public readonly array $crons,
        public readonly ?string $description = null,
        public readonly ?string $clientId = null,
        public readonly ?string $applicationId = null,
        public readonly string $timezone = 'UTC',
        public readonly mixed $payload = null,
        public readonly bool $concurrent = false,
        public readonly bool $tracksCompletion = false,
        public readonly ?int $timeoutSeconds = null,
        public readonly int $deliveryMaxAttempts = 3,
        public readonly ?string $targetUrl = null,
    ) {}

    /**
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        $out = [
            'code' => $this->code,
            'name' => $this->name,
            'crons' => $this->crons,
            'timezone' => $this->timezone,
            'concurrent' => $this->concurrent,
            'tracksCompletion' => $this->tracksCompletion,
            'deliveryMaxAttempts' => $this->deliveryMaxAttempts,
        ];
        if ($this->description !== null) $out['description'] = $this->description;
        if ($this->clientId !== null) $out['clientId'] = $this->clientId;
        if ($this->applicationId !== null) $out['applicationId'] = $this->applicationId;
        if ($this->payload !== null) $out['payload'] = $this->payload;
        if ($this->timeoutSeconds !== null) $out['timeoutSeconds'] = $this->timeoutSeconds;
        if ($this->targetUrl !== null) $out['targetUrl'] = $this->targetUrl;
        return $out;
    }
}
