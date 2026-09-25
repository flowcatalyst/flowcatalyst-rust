<?php

declare(strict_types=1);

namespace FlowCatalyst\Auth\Contracts;

/**
 * Resolves a principal's effective permissions from its roles.
 *
 * The token guard calls this after validating an access token, passing the
 * principal's role names (lifted from the token). Bind your own implementation
 * in the app's service provider to resolve permissions however you like — your
 * own DB, a cache, a downstream service — and the SDK will use it everywhere
 * (hasPermissionTo(), can(), policies).
 *
 * The SDK ships two implementations:
 *   - {@see \FlowCatalyst\Auth\Support\ApiMePermissionResolver}  (default) —
 *     fetches the server-resolved set from GET /api/me, cached by token.
 *   - {@see \FlowCatalyst\Auth\Rbac\CataloguePermissionResolver} — offline,
 *     maps roles → permissions via a static RbacCatalogue.
 */
interface PermissionResolver
{
    /**
     * @param array<int,string> $roles The principal's role names (from the token).
     * @param string|null       $token The raw access token, for server-backed
     *                                  resolvers; ignore it in pure role→perm maps.
     * @return array<int,string> The effective permission strings (FlowCatalyst
     *                           wildcard form, e.g. "platform:messaging:event:*").
     */
    public function resolve(array $roles, ?string $token = null): array;
}
