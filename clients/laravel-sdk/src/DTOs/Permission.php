<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs;

/**
 * A granted permission as seen in the platform's permissions catalogue.
 *
 * `permission` is the full string (`application:context:aggregate:action`)
 * and the parsed segments are exposed individually.
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
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        return new self(
            permission: (string) $data['permission'],
            application: (string) ($data['application'] ?? ''),
            context: (string) ($data['context'] ?? ''),
            aggregate: (string) ($data['aggregate'] ?? ''),
            action: (string) ($data['action'] ?? ''),
            description: (string) ($data['description'] ?? ''),
        );
    }

    /**
     * @return array<string, string>
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
        ];
    }
}
