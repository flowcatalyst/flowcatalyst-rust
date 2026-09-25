<?php

declare(strict_types=1);

namespace FlowCatalyst\Attributes;

use Attribute;
use FlowCatalyst\Enums\DispatchMode;

/**
 * Marks a class as a subscription definition for FlowCatalyst.
 *
 * Usage:
 * ```php
 * #[AsSubscription(
 *     code: 'order-processor',
 *     name: 'Order Processor',
 *     target: '/webhooks/orders',  // path → resolved against APP_URL at sync
 *     description: 'Processes new orders',
 *     clientScoped: true,  // Set to true for client-scoped subscriptions
 *     eventTypes: ['myapp:orders:order:created', 'myapp:orders:order:updated'],
 *     connectionCode: 'orders-webhook',
 *     queue: 'orders',
 *     dispatchPoolCode: 'default',
 *     mode: DispatchMode::BLOCK_ON_ERROR
 * )]
 * class OrderProcessorSubscription {}
 * ```
 */
#[Attribute(Attribute::TARGET_CLASS)]
final class AsSubscription
{
    /**
     * @param string $code Unique subscription code
     * @param string $name Human-friendly name
     * @param string $target Where the platform delivers this subscription's
     *        events. Either an absolute URL, sent as-is, or a path
     *        (`/webhooks/orders`), which the sync resolves against
     *        `flowcatalyst.subscriptions.target_base_url`, falling back to
     *        `app.url`. Prefer a path: the attribute is compiled into the
     *        codebase while the host differs per environment, and resolving at
     *        sync time (not scan time) means a definition cache built in CI
     *        still gets the deploying environment's host.
     * @param string $connectionCode Code of the connection that delivers this
     *        subscription. The code, not the id: an id is minted per
     *        environment, a code is the same everywhere. The platform resolves
     *        it at sync time. A bare code names a connection owned by THIS
     *        application; set `sharedConnection: true` when it names a
     *        shared (application-less) connection instead.
     * @param string $queue Queue name for message routing
     * @param string $dispatchPoolCode Dispatch pool code for rate limiting
     * @param string|null $description Subscription description
     * @param bool $clientScoped Whether this subscription is client-scoped
     * @param string[] $eventTypes Array of event type codes to subscribe to
     * @param array<array{key: string, value: string}> $customConfig Custom configuration entries
     * @param int|null $maxAgeSeconds Maximum age in seconds for dispatch jobs
     * @param int|null $delaySeconds Delay before first dispatch attempt
     * @param int|null $sequence Sequence number for ordering
     * @param DispatchMode|string|null $mode Dispatch mode (IMMEDIATE or BLOCK_ON_ERROR)
     * @param int|null $timeoutSeconds Timeout for target response
     * @param int|null $maxRetries Maximum retry attempts
     * @param bool|null $dataOnly If true, only send event data (no envelope)
     * @param string|null $application Application code this subscription belongs
     *        to. Overrides the global `application_code` / `application_map`
     *        during sync — set it when one codebase defines subscriptions for
     *        more than one application. Null = resolve from the namespace map /
     *        default.
     * @param bool $sharedConnection Whether `connectionCode` names a SHARED
     *        (application-less) connection rather than one owned by this
     *        application. Only meaningful together with `connectionCode`;
     *        the payload only carries this field when true.
     * @param string|null $client The FlowCatalyst client (by identifier
     *        slug) this subscription is scoped to. Null = the config default
     *        `flowcatalyst.client` (single-tenant apps), and null there too
     *        means global (no client). For a multi-tenant application, don't
     *        set this on the attribute — build one `SyncDefinitionSet` per
     *        client instead (see `SyncDefinitionSet::forClient()`).
     */
    public function __construct(
        public readonly string $code,
        public readonly string $name,
        public readonly string $target,
        public readonly string $connectionCode,
        public readonly string $queue,
        public readonly string $dispatchPoolCode,
        public readonly ?string $description = null,
        public readonly bool $clientScoped = false,
        public readonly array $eventTypes = [],
        public readonly array $customConfig = [],
        public readonly ?int $maxAgeSeconds = null,
        public readonly ?int $delaySeconds = null,
        public readonly ?int $sequence = null,
        public readonly DispatchMode|string|null $mode = null,
        public readonly ?int $timeoutSeconds = null,
        public readonly ?int $maxRetries = null,
        public readonly ?bool $dataOnly = null,
        public readonly ?string $application = null,
        public readonly bool $sharedConnection = false,
        public readonly ?string $client = null,
    ) {}

    /**
     * Convert to array format for API sync.
     */
    public function toArray(): array
    {
        $mode = $this->mode;
        if ($mode instanceof DispatchMode) {
            $mode = $mode->value;
        }

        // Convert event types to binding format
        $eventTypeBindings = array_map(
            fn(string $code) => ['eventTypeCode' => $code],
            $this->eventTypes
        );

        $data = [
            'code' => $this->code,
            'name' => $this->name,
            'target' => $this->target,
            'clientScoped' => $this->clientScoped,
            'eventTypes' => $eventTypeBindings ?: null,
            'connectionCode' => $this->connectionCode,
            'queue' => $this->queue,
            'dispatchPoolCode' => $this->dispatchPoolCode,
        ];

        if ($this->description !== null) {
            $data['description'] = $this->description;
        }
        if (!empty($this->customConfig)) {
            $data['customConfig'] = $this->customConfig;
        }
        if ($this->maxAgeSeconds !== null) {
            $data['maxAgeSeconds'] = $this->maxAgeSeconds;
        }
        if ($this->delaySeconds !== null) {
            $data['delaySeconds'] = $this->delaySeconds;
        }
        if ($this->sequence !== null) {
            $data['sequence'] = $this->sequence;
        }
        if ($mode !== null) {
            $data['mode'] = $mode;
        }
        if ($this->timeoutSeconds !== null) {
            $data['timeoutSeconds'] = $this->timeoutSeconds;
        }
        if ($this->maxRetries !== null) {
            $data['maxRetries'] = $this->maxRetries;
        }
        if ($this->dataOnly !== null) {
            $data['dataOnly'] = $this->dataOnly;
        }
        if ($this->sharedConnection) {
            $data['sharedConnection'] = true;
        }

        return $data;
    }
}
