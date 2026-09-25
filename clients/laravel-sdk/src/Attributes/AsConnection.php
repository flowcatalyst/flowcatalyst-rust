<?php

declare(strict_types=1);

namespace FlowCatalyst\Attributes;

use Attribute;

/**
 * Marks a class as a connection definition for FlowCatalyst.
 *
 * A connection is application-owned: the platform assigns the connection's
 * service account itself (the application's provisioned one), so the
 * definition carries nothing environment-specific — no service account id,
 * no secret. It exists purely to give a subscription's `connectionCode`
 * something to resolve.
 *
 * Usage:
 * ```php
 * #[AsConnection(
 *     code: 'orders-webhook',
 *     name: 'Orders Webhook',
 *     description: 'Delivers order events to the orders service',
 * )]
 * class OrdersWebhookConnection {}
 * ```
 */
#[Attribute(Attribute::TARGET_CLASS)]
final class AsConnection
{
    /**
     * @param string $code Unique connection code (stable across environments,
     *        unlike its id) — what a subscription's `connectionCode` names.
     * @param string $name Human-friendly name
     * @param string|null $description Connection description
     * @param string|null $externalId Your own system's identifier for this
     *        connection, if any
     * @param string|null $application Application code this connection
     *        belongs to. Overrides the global `application_code` /
     *        `application_map` during sync — set it when one codebase
     *        defines connections for more than one application. Null =
     *        resolve from the namespace map / default.
     * @param string|null $client The FlowCatalyst client (by identifier
     *        slug) this connection is scoped to. Null = the config default
     *        `flowcatalyst.client` (single-tenant apps), and null there too
     *        means global (no client). For a multi-tenant application, don't
     *        set this on the attribute — build one `SyncDefinitionSet` per
     *        client instead (see `SyncDefinitionSet::forClient()`).
     */
    public function __construct(
        public readonly string $code,
        public readonly string $name,
        public readonly ?string $description = null,
        public readonly ?string $externalId = null,
        public readonly ?string $application = null,
        public readonly ?string $client = null,
    ) {}

    /**
     * Convert to array format for API sync. `application` and `client` are
     * routing axes only (resolved by the scanner into `_application` /
     * `client` on the scanned row) and never appear here.
     *
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        $data = [
            'code' => $this->code,
            'name' => $this->name,
        ];

        if ($this->description !== null) {
            $data['description'] = $this->description;
        }
        if ($this->externalId !== null) {
            $data['externalId'] = $this->externalId;
        }

        return $data;
    }
}
