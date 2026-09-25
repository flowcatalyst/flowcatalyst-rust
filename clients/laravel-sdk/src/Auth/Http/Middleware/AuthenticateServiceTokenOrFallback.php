<?php

declare(strict_types=1);

namespace FlowCatalyst\Auth\Http\Middleware;

use Closure;
use FlowCatalyst\Auth\FlowCatalystTokenGuard;
use Illuminate\Http\Request;
use Illuminate\Pipeline\Pipeline;
use Symfony\Component\HttpFoundation\Response;

/**
 * Authenticate a FlowCatalyst service-account (or user) access token, falling
 * back to an existing middleware chain — e.g. Passport's
 * `['client', 'passport.app.identity']` — when the request is NOT a
 * FlowCatalyst token.
 *
 *   Route::post('/api/...', ...)->middleware('fc.or-passport');
 *
 * Behaviour:
 *   - Valid FlowCatalyst token → the resolved {@see \FlowCatalyst\Auth\FlowCatalystAuthenticatable}
 *     is set on the auth guard (so auth()->user(), $request->user(), policies
 *     and @can all work) and the fallback chain NEVER runs.
 *   - Anything else → the configured fallback chain runs unchanged, exactly as
 *     if this middleware weren't there.
 *
 * The fallback chain is configured (not hard-coded) via
 * `flowcatalyst.oidc.token_fallback_middleware`, e.g.:
 *   'token_fallback_middleware' => ['client', 'passport.app.identity'],
 * An empty list means "no fallback" (FlowCatalyst-only; unauthenticated
 * requests simply pass through and a downstream guard middleware decides).
 */
final class AuthenticateServiceTokenOrFallback
{
    public function __construct(private readonly FlowCatalystTokenGuard $guard)
    {
    }

    public function handle(Request $request, Closure $next): Response
    {
        $user = $this->guard->resolve($request);
        if ($user !== null) {
            auth()->setUser($user);
            $request->setUserResolver(static fn () => $user);
            $request->attributes->set('fc.principal', $user->fcUser());
            return $next($request);
        }

        $fallback = array_values((array) config('flowcatalyst.oidc.token_fallback_middleware', []));
        if ($fallback === []) {
            return $next($request);
        }

        return app(Pipeline::class)
            ->send($request)
            ->through($fallback)
            ->then(static fn (Request $req) => $next($req));
    }
}
