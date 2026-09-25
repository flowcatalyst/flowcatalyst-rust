<?php

declare(strict_types=1);

namespace FlowCatalyst\Sync;

/**
 * Represents a subscription definition for syncing to FlowCatalyst.
 *
 * Can be used directly or converted to array for the sync API.
 */
final class SubscriptionDefinition
{
    /**
     * @param string $code Unique subscription code
     * @param string $name Human-readable name
     * @param string $target Delivery URL — absolute, or a path the sync
     *        resolves against flowcatalyst.subscriptions.target_base_url / app.url
     * @param string $connectionCode Code of the connection that delivers this
     *        subscription — stable across environments, unlike its id. A
     *        bare code names a connection owned by THIS application; set
     *        `sharedConnection: true` when it names a shared
     *        (application-less) connection instead.
     * @param string $queue Queue name for delivery
     * @param string $dispatchPoolCode Code of the dispatch pool to use
     * @param string|null $applicationCode Application code this subscription belongs to
     * @param string|null $description Subscription description
     * @param bool $clientScoped Whether this subscription is client-scoped
     * @param string|null $eventTypeCode Code of the event type to subscribe to (resolved to ID by platform)
     * @param int $maxRetries Maximum retry attempts (default: 3)
     * @param int $retryDelaySeconds Delay between retries in seconds (default: 60)
     * @param int $timeoutSeconds Webhook timeout in seconds (default: 30)
     * @param bool $active Whether the subscription is active (default: true)
     * @param bool $sharedConnection Whether `connectionCode` names a shared
     *        (application-less) connection. Only sent when true.
     * @param string|null $client FlowCatalyst client (identifier slug) this
     *        subscription is scoped to. Null = global. Unlike the attribute
     *        default, this is never overridden by `flowcatalyst.client` —
     *        set it explicitly, or scope the whole set via
     *        `SyncDefinitionSet::forClient()` instead.
     */
    public function __construct(
        public readonly string $code,
        public readonly string $name,
        public readonly string $target,
        public readonly string $connectionCode,
        public readonly string $queue,
        public readonly string $dispatchPoolCode,
        public readonly ?string $applicationCode = null,
        public readonly ?string $description = null,
        public readonly bool $clientScoped = false,
        public readonly ?string $eventTypeCode = null,
        public readonly int $maxRetries = 3,
        public readonly int $retryDelaySeconds = 60,
        public readonly int $timeoutSeconds = 30,
        public readonly bool $active = true,
        public readonly bool $sharedConnection = false,
        public readonly ?string $client = null,
    ) {}

    /**
     * Create a new subscription definition with fluent syntax.
     */
    public static function make(
        string $code,
        string $name,
        string $target,
        string $connectionCode,
        string $queue,
        string $dispatchPoolCode
    ): self {
        return new self($code, $name, $target, $connectionCode, $queue, $dispatchPoolCode);
    }

    /**
     * Create a copy with a different description.
     */
    public function withDescription(string $description): self
    {
        return new self(
            code: $this->code,
            name: $this->name,
            connectionCode: $this->connectionCode,
            queue: $this->queue,
            dispatchPoolCode: $this->dispatchPoolCode,
            applicationCode: $this->applicationCode,
            description: $description,
            clientScoped: $this->clientScoped,
            eventTypeCode: $this->eventTypeCode,
            maxRetries: $this->maxRetries,
            retryDelaySeconds: $this->retryDelaySeconds,
            timeoutSeconds: $this->timeoutSeconds,
            active: $this->active,
            target: $this->target,
            sharedConnection: $this->sharedConnection,
            client: $this->client,
        );
    }

    /**
     * Create a copy that delivers to $target — an absolute URL, or a path
     * resolved against the configured base URL at sync time.
     */
    public function withTarget(string $target): self
    {
        return new self(
            code: $this->code,
            name: $this->name,
            connectionCode: $this->connectionCode,
            queue: $this->queue,
            dispatchPoolCode: $this->dispatchPoolCode,
            applicationCode: $this->applicationCode,
            description: $this->description,
            clientScoped: $this->clientScoped,
            eventTypeCode: $this->eventTypeCode,
            maxRetries: $this->maxRetries,
            retryDelaySeconds: $this->retryDelaySeconds,
            timeoutSeconds: $this->timeoutSeconds,
            active: $this->active,
            target: $target,
            sharedConnection: $this->sharedConnection,
            client: $this->client,
        );
    }

    /**
     * Create a copy that is client-scoped.
     */
    public function clientScoped(bool $clientScoped = true): self
    {
        return new self(
            code: $this->code,
            name: $this->name,
            connectionCode: $this->connectionCode,
            queue: $this->queue,
            dispatchPoolCode: $this->dispatchPoolCode,
            applicationCode: $this->applicationCode,
            description: $this->description,
            clientScoped: $clientScoped,
            eventTypeCode: $this->eventTypeCode,
            maxRetries: $this->maxRetries,
            retryDelaySeconds: $this->retryDelaySeconds,
            timeoutSeconds: $this->timeoutSeconds,
            active: $this->active,
            target: $this->target,
            sharedConnection: $this->sharedConnection,
            client: $this->client,
        );
    }

    /**
     * Create a copy that subscribes to a specific event type.
     */
    public function forEventType(string $eventTypeCode): self
    {
        return new self(
            code: $this->code,
            name: $this->name,
            connectionCode: $this->connectionCode,
            queue: $this->queue,
            dispatchPoolCode: $this->dispatchPoolCode,
            applicationCode: $this->applicationCode,
            description: $this->description,
            clientScoped: $this->clientScoped,
            eventTypeCode: $eventTypeCode,
            maxRetries: $this->maxRetries,
            retryDelaySeconds: $this->retryDelaySeconds,
            timeoutSeconds: $this->timeoutSeconds,
            active: $this->active,
            target: $this->target,
            sharedConnection: $this->sharedConnection,
            client: $this->client,
        );
    }

    /**
     * Create a copy with different retry settings.
     */
    public function withRetry(int $maxRetries, int $delaySeconds = 60): self
    {
        return new self(
            code: $this->code,
            name: $this->name,
            connectionCode: $this->connectionCode,
            queue: $this->queue,
            dispatchPoolCode: $this->dispatchPoolCode,
            applicationCode: $this->applicationCode,
            description: $this->description,
            clientScoped: $this->clientScoped,
            eventTypeCode: $this->eventTypeCode,
            maxRetries: $maxRetries,
            retryDelaySeconds: $delaySeconds,
            timeoutSeconds: $this->timeoutSeconds,
            active: $this->active,
            target: $this->target,
            sharedConnection: $this->sharedConnection,
            client: $this->client,
        );
    }

    /**
     * Create a copy with a different timeout.
     */
    public function withTimeout(int $seconds): self
    {
        return new self(
            code: $this->code,
            name: $this->name,
            connectionCode: $this->connectionCode,
            queue: $this->queue,
            dispatchPoolCode: $this->dispatchPoolCode,
            applicationCode: $this->applicationCode,
            description: $this->description,
            clientScoped: $this->clientScoped,
            eventTypeCode: $this->eventTypeCode,
            maxRetries: $this->maxRetries,
            retryDelaySeconds: $this->retryDelaySeconds,
            timeoutSeconds: $seconds,
            active: $this->active,
            target: $this->target,
            sharedConnection: $this->sharedConnection,
            client: $this->client,
        );
    }

    /**
     * Create a copy that is inactive.
     */
    public function inactive(): self
    {
        return new self(
            code: $this->code,
            name: $this->name,
            connectionCode: $this->connectionCode,
            queue: $this->queue,
            dispatchPoolCode: $this->dispatchPoolCode,
            applicationCode: $this->applicationCode,
            description: $this->description,
            clientScoped: $this->clientScoped,
            eventTypeCode: $this->eventTypeCode,
            maxRetries: $this->maxRetries,
            retryDelaySeconds: $this->retryDelaySeconds,
            timeoutSeconds: $this->timeoutSeconds,
            active: false,
            target: $this->target,
            sharedConnection: $this->sharedConnection,
            client: $this->client,
        );
    }

    /**
     * Create a copy scoped to a specific client.
     */
    public function forClient(string $client): self
    {
        return new self(
            code: $this->code,
            name: $this->name,
            connectionCode: $this->connectionCode,
            queue: $this->queue,
            dispatchPoolCode: $this->dispatchPoolCode,
            applicationCode: $this->applicationCode,
            description: $this->description,
            clientScoped: $this->clientScoped,
            eventTypeCode: $this->eventTypeCode,
            maxRetries: $this->maxRetries,
            retryDelaySeconds: $this->retryDelaySeconds,
            timeoutSeconds: $this->timeoutSeconds,
            active: $this->active,
            target: $this->target,
            sharedConnection: $this->sharedConnection,
            client: $client,
        );
    }

    /**
     * Convert to array for the sync API. `client` is a routing field (read
     * by the synchronizer to pick which call this row belongs to) rather
     * than a payload field of the subscription itself, but is included here
     * — unlike the attribute's `toArray()` — because nothing else resolves
     * it for a programmatically-built definition.
     *
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        $data = [
            'code' => $this->code,
            'name' => $this->name,
            'target' => $this->target,
            'connectionCode' => $this->connectionCode,
            'queue' => $this->queue,
            'dispatchPoolCode' => $this->dispatchPoolCode,
            'clientScoped' => $this->clientScoped,
            'maxRetries' => $this->maxRetries,
            'retryDelaySeconds' => $this->retryDelaySeconds,
            'timeoutSeconds' => $this->timeoutSeconds,
            'active' => $this->active,
        ];

        if ($this->applicationCode !== null) {
            $data['applicationCode'] = $this->applicationCode;
        }

        if ($this->description !== null) {
            $data['description'] = $this->description;
        }

        if ($this->eventTypeCode !== null) {
            $data['eventTypeCode'] = $this->eventTypeCode;
        }

        if ($this->sharedConnection) {
            $data['sharedConnection'] = true;
        }

        if ($this->client !== null) {
            $data['client'] = $this->client;
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
            code: $data['code'],
            name: $data['name'],
            connectionCode: $data['connectionCode'],
            queue: $data['queue'],
            dispatchPoolCode: $data['dispatchPoolCode'],
            applicationCode: $data['applicationCode'] ?? null,
            description: $data['description'] ?? null,
            clientScoped: $data['clientScoped'] ?? false,
            eventTypeCode: $data['eventTypeCode'] ?? null,
            maxRetries: $data['maxRetries'] ?? 3,
            retryDelaySeconds: $data['retryDelaySeconds'] ?? 60,
            timeoutSeconds: $data['timeoutSeconds'] ?? 30,
            active: $data['active'] ?? true,
            target: $data['target'] ?? $data['endpoint'],
            sharedConnection: $data['sharedConnection'] ?? false,
            client: $data['client'] ?? null,
        );
    }
}
