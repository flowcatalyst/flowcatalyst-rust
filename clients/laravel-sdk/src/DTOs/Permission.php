<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs;

/**
 * A granted permission as seen in the platform's permissions catalogue.
 *
 * `permission` is the full string (`application:context:aggregate:action`)
 * and the parsed segments are exposed individually. The platform returns
 * `{permission, name, description, category}`; the segments are parsed from
 * `permission` when the response does not carry them.
 */
final class Permission
{
    public function __construct(
        public readonly string $permission,
        public readonly string $application,
        public readonly string $context,
        public readonly string $aggregate,
        public readonly string $action,
        public readonly string $description,
        public readonly ?string $name = null,
        public readonly ?string $category = null,
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        $permission = (string) $data['permission'];
        $segments = array_pad(explode(':', $permission, 4), 4, '');

        return new self(
            permission: $permission,
            application: (string) ($data['application'] ?? $segments[0]),
            context: (string) ($data['context'] ?? $segments[1]),
            aggregate: (string) ($data['aggregate'] ?? $segments[2]),
            action: (string) ($data['action'] ?? $segments[3]),
            description: (string) ($data['description'] ?? ''),
            name: isset($data['name']) ? (string) $data['name'] : null,
            category: isset($data['category']) ? (string) $data['category'] : null,
        );
    }

    /**
     * @return array<string, string|null>
     */
    public function toArray(): array
    {
        return [
            'permission' => $this->permission,
            'application' => $this->application,
            'context' => $this->context,
            'aggregate' => $this->aggregate,
            'action' => $this->action,
            'description' => $this->description,
            'name' => $this->name,
            'category' => $this->category,
        ];
    }
}
