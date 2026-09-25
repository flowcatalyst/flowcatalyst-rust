<?php

declare(strict_types=1);

namespace FlowCatalyst\Auth\Http\Middleware;

use Closure;
use FlowCatalyst\Auth\DefaultOidcUserHandler;
use FlowCatalyst\Auth\DTOs\FlowCatalystUser;
use FlowCatalyst\Auth\Rbac\RbacCatalogue;
use FlowCatalyst\Auth\Support\AccessTokenValidator;
use FlowCatalyst\Auth\Support\SessionFreshnessGuard;
use Illuminate\Http\Request;
use Symfony\Component\HttpFoundation\Response;

/**
 * Resolves the current request's FlowCatalyst principal.
 *
 *   1. `Authorization: Bearer <token>` → validates against JWKS (RS256).
 *   2. Otherwise reads the existing session-stored principal (set by the
 *      OIDC callback flow), capped to its OWN access token's real expiry —
 *      see {@see SessionFreshnessGuard}.
 *   3. Applies the RBAC catalogue (if registered) to populate
 *      `principal->permissions`.
 *   4. Stashes the principal on the request via
 *      `$request->attributes->set('fc.principal', $user)`.
 *
 * The guard middleware ({@see RequireSession}, {@see RequireBearer},
 * {@see RequireAuth}) read that attribute to decide whether to admit the
 * request, redirect, or 401.
 *
 * Bearer wins over session on the same request: an explicit Authorization
 * header is never silently downgraded to whatever session cookie the browser
 * sent.
 */
final class AuthenticateFc
{
    public function __construct(
        private readonly AccessTokenValidator $validator,
        private readonly ?RbacCatalogue $rbac = null,
        private readonly ?SessionFreshnessGuard $freshness = null,
    ) {}

    public function handle(Request $request, Closure $next): Response
    {
        $principal = $this->resolve($request);
        if ($principal !== null) {
            if ($this->rbac !== null) {
                $principal = $principal->withRbac($this->rbac);
            }
            $request->attributes->set('fc.principal', $principal);
        }
        return $next($request);
    }

    private function resolve(Request $request): ?FlowCatalystUser
    {
        $bearer = $this->readBearer($request);
        if ($bearer !== null) {
            return $this->validator->validate($bearer);
        }

        // Session-stored principal (set by OidcAuthController on callback).
        // Goes through DefaultOidcUserHandler::getCurrentUser() rather than a
        // raw session()->get() + instanceof check: Laravel's session
        // 'serialization' can be 'json' (a recommended hardening — no
        // "gadget chain" risk — increasingly the default in new apps), under
        // which a stored object always comes back as a plain array, never a
        // reconstructed instance. getCurrentUser() is the one place that
        // knows how to rehydrate it regardless of serialization mode.
        if (!$request->hasSession()) {
            return null;
        }
        $stored = DefaultOidcUserHandler::getCurrentUser();
        if ($stored === null) {
            return null;
        }

        // No freshness guard wired (e.g. a unit test constructing this
        // middleware directly) — trust the stored principal as-is rather
        // than silently dropping every session. Note: this is NOT the same
        // as ensureFresh() returning null (a failed refresh), which must
        // still clear the session — hence the explicit null check on
        // $this->freshness itself, not a `??` on its result.
        $fresh = $this->freshness === null ? $stored : $this->freshness->ensureFresh($stored);

        return $fresh?->withMechanism('session');
    }

    private function readBearer(Request $request): ?string
    {
        $raw = $request->headers->get('Authorization');
        if (!is_string($raw)) {
            return null;
        }
        if (preg_match('/^Bearer\s+(.+)$/i', trim($raw), $m) === 1) {
            return trim($m[1]);
        }
        return null;
    }
}
