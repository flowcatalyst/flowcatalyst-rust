<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

/**
 * One entry in the POST /api/applications/{appCode}/processes/sync payload.
 *
 * `code` should be the full three-segment code
 * (`{application}:{subdomain}:{processName}`). `body` carries the diagram
 * source verbatim — typically Mermaid. The platform never inspects it.
 */
final class SyncProcessEntry
{
    /**
     * @param string[] $tags
     */
    public function __construct(
        public readonly string $code,
        public readonly string $name,
        public readonly string $body = '',
        public readonly ?string $description = null,
        public readonly string $diagramType = 'mermaid',
        public readonly array $tags = [],
    ) {}

    /**
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        $payload = [
            'code' => $this->code,
            'name' => $this->name,
            'body' => $this->body,
            'diagramType' => $this->diagramType,
            'tags' => $this->tags,
        ];
        if ($this->description !== null) {
            $payload['description'] = $this->description;
        }
        return $payload;
    }
}
