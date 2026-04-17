<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs;

use FlowCatalyst\Enums\EventTypeStatus;

/**
 * An event type.
 *
 * The full `code` is formatted `{application}:{subdomain}:{aggregate}:{event}`,
 * and the parsed segments are exposed individually.
 */
final class EventType
{
    /**
     * @param SpecVersion[] $specVersions
     */
    public function __construct(
        public readonly string $id,
        public readonly string $code,
        public readonly string $name,
        public readonly EventTypeStatus $status,
        public readonly string $application,
        public readonly string $subdomain,
        public readonly string $aggregate,
        public readonly string $event,
        public readonly array $specVersions,
        public readonly string $createdAt,
        public readonly string $updatedAt,
        public readonly ?string $description = null,
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        /** @var array<int, array<string, mixed>> $specRows */
        $specRows = $data['specVersions'] ?? [];
        return new self(
            id: (string) $data['id'],
            code: (string) $data['code'],
            name: (string) $data['name'],
            status: EventTypeStatus::tryFrom((string) ($data['status'] ?? 'CURRENT'))
                ?? EventTypeStatus::CURRENT,
            application: (string) ($data['application'] ?? ''),
            subdomain: (string) ($data['subdomain'] ?? ''),
            aggregate: (string) ($data['aggregate'] ?? ''),
            event: (string) ($data['event'] ?? ''),
            specVersions: array_map(
                fn(array $sv) => SpecVersion::fromArray($sv),
                $specRows,
            ),
            createdAt: (string) ($data['createdAt'] ?? ''),
            updatedAt: (string) ($data['updatedAt'] ?? ''),
            description: isset($data['description']) ? (string) $data['description'] : null,
        );
    }

    /**
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        return [
            'id' => $this->id,
            'code' => $this->code,
            'name' => $this->name,
            'status' => $this->status->value,
            'application' => $this->application,
            'subdomain' => $this->subdomain,
            'aggregate' => $this->aggregate,
            'event' => $this->event,
            'specVersions' => array_map(fn(SpecVersion $sv) => $sv->toArray(), $this->specVersions),
            'createdAt' => $this->createdAt,
            'updatedAt' => $this->updatedAt,
            'description' => $this->description,
        ];
    }
}
