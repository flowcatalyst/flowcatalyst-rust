<?php

declare(strict_types=1);

namespace FlowCatalyst\Attributes;

use Attribute;

/**
 * Marks a class as an event type definition for FlowCatalyst.
 *
 * Usage:
 * ```php
 * #[AsEventType(
 *     subdomain: 'orders',
 *     aggregate: 'order',
 *     event: 'created',
 *     name: 'Order Created',
 *     description: 'Emitted when a new order is placed',
 *     clientScoped: true  // Set to true if events are client-specific
 * )]
 * class OrderCreatedEvent {}
 * ```
 *
 * The application code is automatically added from your config when synced.
 * For example, if your app code is "myapp", the event type becomes "myapp:orders:order:created".
 * Set `application:` to override it per definition (see the constructor).
 *
 * Code format: {application}:{subdomain}:{aggregate}:{event}
 */
#[Attribute(Attribute::TARGET_CLASS)]
final class AsEventType
{
    /**
     * @param string $subdomain Subdomain within the application (e.g., "orders", "logistics")
     * @param string $aggregate Aggregate/entity name (e.g., "order", "trip")
     * @param string $event Event name (e.g., "created", "updated", "started")
     * @param string $name Human-friendly name
     * @param string|null $description Event type description
     * @param bool $clientScoped Whether events of this type are scoped to clients
     * @param string|null $application Application code this event type belongs to.
     *        Overrides the global `application_code` / `application_map` during
     *        sync — set it when one codebase defines event types for more than
     *        one application. Null = resolve from the namespace map / default.
     */
    public function __construct(
        public readonly string $subdomain,
        public readonly string $aggregate,
        public readonly string $event,
        public readonly string $name,
        public readonly ?string $description = null,
        public readonly bool $clientScoped = false,
        public readonly ?string $application = null,
    ) {}

    /**
     * Convert to array format for API sync.
     */
    public function toArray(): array
    {
        $data = [
            'subdomain' => $this->subdomain,
            'aggregate' => $this->aggregate,
            'event' => $this->event,
            'name' => $this->name,
            'clientScoped' => $this->clientScoped,
        ];

        if ($this->description !== null) {
            $data['description'] = $this->description;
        }

        return $data;
    }
}
