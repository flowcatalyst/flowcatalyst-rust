<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs;

/**
 * An event-type subscription binding.
 *
 * `eventTypeCode` may include wildcards (e.g. `orders:*:*:created`).
 * `filter` is an optional expression evaluated against event payloads.
 */
final class EventTypeBinding
{
    public function __construct(
        public readonly string $eventTypeCode,
        public readonly ?string $filter = null,
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        return new self(
            eventTypeCode: (string) $data['eventTypeCode'],
            filter: isset($data['filter']) ? (string) $data['filter'] : null,
        );
    }

    /**
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        $payload = ['eventTypeCode' => $this->eventTypeCode];
        if ($this->filter !== null) {
            $payload['filter'] = $this->filter;
        }
        return $payload;
    }
}
