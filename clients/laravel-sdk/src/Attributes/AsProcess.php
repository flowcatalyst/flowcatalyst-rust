<?php

declare(strict_types=1);

namespace FlowCatalyst\Attributes;

use Attribute;

/**
 * Marks a class as a process documentation definition for FlowCatalyst.
 *
 * Usage:
 * ```php
 * #[AsProcess(
 *     subdomain: 'fulfilment',
 *     processName: 'shipment-flow',
 *     name: 'Shipment Flow',
 *     description: 'How a fulfilment becomes a dispatched shipment',
 *     body: <<<MERMAID
 *         graph TD
 *           A[Fulfilment Created] --> B[Enrich locations]
 *           B --> C[Build shipment]
 *           C --> D[Shipment Created]
 *         MERMAID,
 *     tags: ['fulfilment', 'core']
 * )]
 * class ShipmentFlowProcess {}
 * ```
 *
 * The application code is automatically added from your config when synced.
 * For example, if your app code is "orders", the process code becomes
 * "orders:fulfilment:shipment-flow". Set `application:` to override it per
 * definition (see the constructor).
 *
 * Code format: {application}:{subdomain}:{processName}
 */
#[Attribute(Attribute::TARGET_CLASS)]
final class AsProcess
{
    /**
     * @param string $subdomain Subdomain within the application (e.g., "fulfilment", "billing")
     * @param string $processName Process slug (e.g., "shipment-flow", "subscription-renewal")
     * @param string $name Human-friendly name
     * @param string $body Diagram source. Typically Mermaid. Stored verbatim.
     * @param string|null $description Short summary
     * @param string $diagramType Defaults to "mermaid"
     * @param string[] $tags Optional tags for grouping
     * @param string|null $application Application code this process belongs to.
     *        Overrides the global `application_code` / `application_map` during
     *        sync — set it when one codebase defines processes for more than
     *        one application. Null = resolve from the namespace map / default.
     */
    public function __construct(
        public readonly string $subdomain,
        public readonly string $processName,
        public readonly string $name,
        public readonly string $body = '',
        public readonly ?string $description = null,
        public readonly string $diagramType = 'mermaid',
        public readonly array $tags = [],
        public readonly ?string $application = null,
    ) {}

    /**
     * Convert to array format for API sync.
     *
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        $data = [
            'subdomain' => $this->subdomain,
            'processName' => $this->processName,
            'name' => $this->name,
            'body' => $this->body,
            'diagramType' => $this->diagramType,
            'tags' => $this->tags,
        ];

        if ($this->description !== null) {
            $data['description'] = $this->description;
        }

        return $data;
    }
}
