<?php

declare(strict_types=1);

namespace FlowCatalyst\Auth\Rbac;

use FlowCatalyst\Auth\Contracts\PermissionResolver;

/**
 * Offline {@see PermissionResolver}: maps roles → permissions via a static
 * {@see RbacCatalogue}. No network, but the catalogue must be kept in sync with
 * the platform's role definitions by the app.
 */
final class CataloguePermissionResolver implements PermissionResolver
{
    public function __construct(private readonly RbacCatalogue $catalogue)
    {
    }

    public function resolve(array $roles, ?string $token = null): array
    {
        return $this->catalogue->resolve($roles);
    }
}
