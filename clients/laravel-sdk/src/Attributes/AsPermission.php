<?php

declare(strict_types=1);

namespace FlowCatalyst\Attributes;

use Attribute;
use FlowCatalyst\DTOs\PermissionInput;

/**
 * Marks a class as a standalone permission definition — a small "factory" that
 * produces a {@see PermissionInput}.
 *
 * Permissions are defined ONCE and linked from many roles, instead of being
 * declared inline on each role:
 *
 * ```php
 * #[AsPermission(context: 'posts', aggregate: 'post', action: 'view')]
 * class ViewPosts {}
 *
 * #[AsRole(name: 'editor', permissions: [ViewPosts::class, EditPosts::class])]
 * class EditorRole {}
 * ```
 *
 * The `application` segment defaults to your app code (FLOWCATALYST_APP_CODE),
 * so you don't repeat it on every permission. The full permission string is
 * `{application}:{context}:{aggregate}:{action}`.
 *
 * On `flowcatalyst:sync` these are seeded into the local Spatie tables and ride
 * up to the platform via the roles that reference them (FlowCatalyst has no
 * standalone "create permission" — permissions exist by being granted to roles).
 */
#[Attribute(Attribute::TARGET_CLASS)]
final class AsPermission
{
    /**
     * @param string $context Bounded context (e.g. "posts", "iam")
     * @param string $aggregate Resource/entity (e.g. "post", "user")
     * @param string $action Operation (e.g. "view", "create", "update", "delete")
     * @param string|null $application Application segment; defaults to the app code at scan time
     * @param string|null $description Human-friendly description
     */
    public function __construct(
        public readonly string $context,
        public readonly string $aggregate,
        public readonly string $action,
        public readonly ?string $application = null,
        public readonly ?string $description = null,
    ) {}

    /**
     * Produce the {@see PermissionInput} this class defines.
     *
     * @param string|null $defaultApplication Used when `application` is not set on the attribute.
     * @throws \InvalidArgumentException when no application can be determined.
     */
    public function toPermissionInput(?string $defaultApplication = null): PermissionInput
    {
        $application = $this->application ?? $defaultApplication;
        if ($application === null || $application === '') {
            throw new \InvalidArgumentException(
                'AsPermission requires an application: set the `application:` argument or '
                . 'configure FLOWCATALYST_APP_CODE so it can be inferred.'
            );
        }

        return new PermissionInput(
            application: $application,
            context: $this->context,
            aggregate: $this->aggregate,
            action: $this->action,
        );
    }

    /**
     * The full permission string ({application}:{context}:{aggregate}:{action}).
     */
    public function toPermissionString(?string $defaultApplication = null): string
    {
        return $this->toPermissionInput($defaultApplication)->toPermissionString();
    }

    /**
     * Convert to the cached/array shape.
     *
     * @return array<string, mixed>
     */
    public function toArray(?string $defaultApplication = null): array
    {
        $data = ['permission' => $this->toPermissionString($defaultApplication)];

        if ($this->description !== null) {
            $data['description'] = $this->description;
        }

        return $data;
    }
}
