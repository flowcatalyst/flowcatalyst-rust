<?php

declare(strict_types=1);

namespace FlowCatalyst\Sync;

/**
 * Represents an event type definition for syncing to FlowCatalyst.
 *
 * Can be used directly or converted to array for the sync API.
 */
final class EventTypeDefinition
{
    /**
     * @param string $name Human-readable name
     * @param string $application Application segment (e.g., 'tower')
     * @param string $subdomain Subdomain segment (e.g., 'logistics')
     * @param string $aggregate Aggregate segment (e.g., 'trip')
     * @param string $event Event segment (e.g., 'created')
     * @param string|null $description Event type description
     * @param bool $clientScoped Whether events of this type are scoped to clients
     */
    public function __construct(
        public readonly string $name,
        public readonly string $application,
        public readonly string $subdomain,
        public readonly string $aggregate,
        public readonly string $event,
        public readonly ?string $description = null,
        public readonly bool $clientScoped = false,
    ) {}

    /**
     * Get the full code in FlowCatalyst format: {application}:{subdomain}:{aggregate}:{event}
     */
    public function getCode(): string
    {
        return "{$this->application}:{$this->subdomain}:{$this->aggregate}:{$this->event}";
    }

    /**
     * Create a new event type definition with fluent syntax.
     */
    public static function make(
        string $name,
        string $application,
        string $subdomain,
        string $aggregate,
        string $event,
        bool $clientScoped = false
    ): self {
        return new self($name, $application, $subdomain, $aggregate, $event, clientScoped: $clientScoped);
    }

    /**
     * Create from dot-separated code (e.g., 'tower.trip.created').
     *
     * Rules:
     * - 3 segments (a.b.c): app=a, subdomain=a, aggregate=b, event=c
     * - 4+ segments (a.b.c.d...): app=a, subdomain=b, aggregate=c, event=d... (rejoin rest with ".")
     */
    public static function fromDotCode(string $code, string $name, bool $clientScoped = false): self
    {
        $segments = explode('.', $code);
        $count = count($segments);

        if ($count < 3) {
            // Not enough segments, use defaults
            $first = $segments[0] ?? 'unknown';
            $second = $segments[1] ?? $first;
            return new self($name, $first, $first, $second, $code, clientScoped: $clientScoped);
        }

        if ($count === 3) {
            // 3 segments: app=first, subdomain=first, aggregate=second, event=third
            return new self(
                $name,
                $segments[0],
                $segments[0],
                $segments[1],
                $segments[2],
                clientScoped: $clientScoped
            );
        }

        // 4+ segments: app=first, subdomain=second, aggregate=third, event=rest joined by "."
        return new self(
            $name,
            $segments[0],
            $segments[1],
            $segments[2],
            implode('.', array_slice($segments, 3)),
            clientScoped: $clientScoped
        );
    }

    /**
     * Create a copy with a different description.
     */
    public function withDescription(string $description): self
    {
        return new self(
            name: $this->name,
            application: $this->application,
            subdomain: $this->subdomain,
            aggregate: $this->aggregate,
            event: $this->event,
            description: $description,
            clientScoped: $this->clientScoped,
        );
    }

    /**
     * Create a copy that is client-scoped.
     */
    public function clientScoped(bool $clientScoped = true): self
    {
        return new self(
            name: $this->name,
            application: $this->application,
            subdomain: $this->subdomain,
            aggregate: $this->aggregate,
            event: $this->event,
            description: $this->description,
            clientScoped: $clientScoped,
        );
    }

    /**
     * Convert to array for the sync API.
     *
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        $data = [
            // Full assembled code is what the sync API consumes
            // (DefinitionSynchronizer reads $row['code']). The individual
            // segments are kept for callers that introspect them.
            'code' => $this->getCode(),
            'name' => $this->name,
            'application' => $this->application,
            'subdomain' => $this->subdomain,
            'aggregate' => $this->aggregate,
            'event' => $this->event,
            'clientScoped' => $this->clientScoped,
        ];

        if ($this->description !== null) {
            $data['description'] = $this->description;
        }

        return $data;
    }

    /**
     * Create from array (e.g., from cached definitions).
     *
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        return new self(
            name: $data['name'],
            application: $data['application'],
            subdomain: $data['subdomain'],
            aggregate: $data['aggregate'],
            event: $data['event'],
            description: $data['description'] ?? null,
            clientScoped: $data['clientScoped'] ?? false,
        );
    }
}
