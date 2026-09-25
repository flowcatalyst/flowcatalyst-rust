<?php

declare(strict_types=1);

namespace FlowCatalyst\Auth\Support;

use FlowCatalyst\Auth\DefaultOidcUserHandler;
use FlowCatalyst\Auth\DTOs\FlowCatalystUser;
use Illuminate\Support\Facades\Log;

/**
 * Caps a session-stored principal's validity to its OWN access token's real
 * expiry — not Laravel's session lifetime, and not the deliberately
 * short-lived ID token — refreshing via the refresh_token when it lapses.
 * Also, opt-in via `oidc.check_revocation`, forces a refresh when the
 * platform reports the principal changed since this token was minted
 * (faster than waiting for natural expiry).
 *
 * Shared by every consumer of the session-stored principal —
 * {@see \FlowCatalyst\Auth\Http\Middleware\AuthenticateFc} (the `fc.*`
 * middleware path) AND {@see \FlowCatalyst\Auth\FlowCatalystSessionGuard}
 * (the native `auth` guard path, e.g. `AUTH_GUARD=fc-session`) — so a
 * session's validity is capped identically regardless of which guard an app
 * uses to check it.
 */
final class SessionFreshnessGuard
{
    public function __construct(
        private readonly TokenRefresher $refresher,
        private readonly ?RevocationChecker $revocationChecker = null,
    ) {
    }

    /**
     * Returns $stored as-is if still fresh, a refreshed principal if it
     * needed (and could be) refreshed, or null (clearing the session) if
     * neither holds up.
     */
    public function ensureFresh(FlowCatalystUser $stored): ?FlowCatalystUser
    {
        $leeway = (int) config('flowcatalyst.oidc.session_refresh_leeway_seconds', 30);
        $expiresAt = $stored->getAccessTokenExpiresAt();
        // Unknown expiry (a session predating this SDK version) is treated
        // as expired too, so it self-heals on the very next request instead
        // of running uncapped indefinitely.
        $expired = $expiresAt === null || time() >= ($expiresAt - $leeway);

        $stale = false;
        if (!$expired && $this->revocationChecker !== null
            && config('flowcatalyst.oidc.check_revocation', false)
            && $stored->accessToken !== null
        ) {
            $issuedAt = $stored->getAccessTokenIssuedAt();
            if ($issuedAt !== null) {
                $stale = $this->revocationChecker->isStale($stored->sub, $stored->accessToken, $issuedAt);
            }
        }

        if (!$expired && !$stale) {
            return $stored;
        }

        try {
            return $this->refresher->refresh($stored);
        } catch (\Throwable $e) {
            Log::warning('FlowCatalyst: mid-session token refresh failed; clearing session', [
                'error' => $e->getMessage(),
            ]);
            session()->forget(DefaultOidcUserHandler::SESSION_KEY);
            return null;
        }
    }
}
