<?php

declare(strict_types=1);

namespace FlowCatalyst\Auth;

use FlowCatalyst\Auth\Contracts\PermissionResolver;
use FlowCatalyst\Auth\Support\AccessTokenValidator;
use Illuminate\Http\Request;

/**
 * Resolves a {@see FlowCatalystAuthenticatable} from a request's Bearer access
 * token (works for both user and service-account / client_credentials tokens).
 *
 * Returns null for any request that does NOT carry a valid FlowCatalyst token
 * — which is exactly what lets it sit in front of Passport: a non-FlowCatalyst
 * caller falls through to the existing auth chain instead of being rejected.
 *
 * Powers the `fc-token` request guard (use `auth:fc-token` or
 * `auth:fc-token,api`) and the {@see Http\Middleware\AuthenticateServiceTokenOrFallback}
 * dispatcher.
 */
final class FlowCatalystTokenGuard
{
    public function __construct(
        private readonly AccessTokenValidator $validator,
        private readonly ?PermissionResolver $permissions = null,
    ) {
    }

    public function resolve(Request $request): ?FlowCatalystAuthenticatable
    {
        $token = $this->bearer($request);
        if ($token === null) {
            return null;
        }

        // Offline JWKS check first: confirms this is a FlowCatalyst token (and
        // yields sub + roles) with no network call, so non-FlowCatalyst tokens
        // fall through to any fallback (e.g. Passport) fast.
        $user = $this->validator->validate($token);
        if ($user === null) {
            return null;
        }

        // Effective permissions come from the pluggable resolver, passed the
        // principal's roles (and the token, for server-backed resolvers).
        if ($this->permissions !== null) {
            $user = $user->withPermissions($this->permissions->resolve($user->getRoles(), $token));
        }

        return new FlowCatalystAuthenticatable($user->withMechanism('token'));
    }

    private function bearer(Request $request): ?string
    {
        $raw = $request->headers->get('Authorization');
        if (! is_string($raw)) {
            return null;
        }
        if (preg_match('/^Bearer\s+(.+)$/i', trim($raw), $m) === 1) {
            return trim($m[1]);
        }
        return null;
    }
}
