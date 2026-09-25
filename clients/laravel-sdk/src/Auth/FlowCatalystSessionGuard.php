<?php

declare(strict_types=1);

namespace FlowCatalyst\Auth;

use FlowCatalyst\Auth\Contracts\PermissionResolver;
use FlowCatalyst\Auth\Support\SessionFreshnessGuard;
use Illuminate\Http\Request;

/**
 * Resolves a stateless {@see FlowCatalystAuthenticatable} from the OIDC session
 * principal (stored by the OIDC callback). This makes a browser-logged-in user
 * a first-class Laravel auth identity with the full Spatie permission surface
 * (`hasPermissionTo`, `hasRole`, `can`) — with NO local users row and NO Spatie
 * tables.
 *
 * Permissions come from the bound {@see PermissionResolver}; with the default
 * {@see Support\TokenScopePermissionResolver} they're read straight off the
 * stored access token's `scope` claim, so the whole flow is dependency-free.
 *
 * Powers the `fc-session` request guard (use `auth:fc-session`, or set it as
 * the default guard). The Bearer/API counterpart is {@see FlowCatalystTokenGuard}.
 */
final class FlowCatalystSessionGuard
{
    public function __construct(
        private readonly ?PermissionResolver $permissions = null,
        private readonly ?SessionFreshnessGuard $freshness = null,
    ) {
    }

    public function resolve(Request $request): ?FlowCatalystAuthenticatable
    {
        if (! $request->hasSession()) {
            return null;
        }

        $user = DefaultOidcUserHandler::getCurrentUser();
        if ($user === null) {
            return null;
        }

        // Cap this principal's validity to its OWN access token's real expiry
        // (refreshing via the refresh_token when it lapses) — the SAME check
        // AuthenticateFc applies, so a session's validity doesn't depend on
        // which guard an app happens to check it through. No freshness guard
        // wired (e.g. a unit test) → trust the stored principal as-is.
        $user = $this->freshness === null ? $user : $this->freshness->ensureFresh($user);
        if ($user === null) {
            return null;
        }

        // Effective permissions from the resolver (default: the stored access
        // token's `scope` claim), passed the principal's roles + access token.
        if ($this->permissions !== null) {
            $user = $user->withPermissions(
                $this->permissions->resolve($user->getRoles(), $user->accessToken),
            );
        }

        return new FlowCatalystAuthenticatable($user->withMechanism('session'));
    }
}
