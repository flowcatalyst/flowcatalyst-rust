<?php

declare(strict_types=1);

namespace FlowCatalyst\Auth\Support;

/**
 * Verifies FlowCatalyst OIDC id_tokens (RS256) using JWKS — the identity
 * proof minted at login and, when the original login's scope included
 * `openid`, again on refresh.
 *
 * Distinct from {@see AccessTokenValidator} in which claims are mandatory:
 * an id_token's `aud` MUST contain this SDK's own OIDC `client_id` — there
 * is no `expected_audience` opt-out here, since the whole principal (roles
 * included) is built from this token.
 *
 * A failed validation is a login failure. Callers MUST NOT fall back to the
 * token's unverified claims — that was the defect this class replaces
 * ({@see \FlowCatalyst\Auth\Http\Controllers\OidcAuthController}'s old
 * `parseIdToken`, which checked only `exp`/`sub`/email-ish claims).
 */
final class IdTokenValidator
{
    public function __construct(
        private readonly JwksCache $jwks,
        private readonly string $baseUrl,
        private readonly string $clientId,
    ) {}

    /**
     * Verify signature, `iss`, `aud`, `exp`, `nbf` (60s leeway, matching the
     * access-token path) and `sub`.
     *
     * @return array<string, mixed>|null the decoded claims on success, or
     *   null on any validation failure.
     */
    public function validate(string $idToken): ?array
    {
        $verified = JwtSignature::verify($idToken, $this->jwks, $this->baseUrl);
        if ($verified === null) {
            return null;
        }
        $payload = $verified['payload'];

        $now = time();
        if (isset($payload['exp']) && is_numeric($payload['exp']) && (int) $payload['exp'] < $now) {
            return null;
        }
        if (isset($payload['nbf']) && is_numeric($payload['nbf']) && (int) $payload['nbf'] > $now + 60) {
            return null;
        }
        if (!isset($payload['sub']) || !is_string($payload['sub'])) {
            return null;
        }

        $issuer = $this->jwks->issuer($this->baseUrl);
        if ($issuer === null || ($payload['iss'] ?? null) !== $issuer) {
            return null;
        }

        $aud = $payload['aud'] ?? null;
        $audMatch = is_string($aud)
            ? $aud === $this->clientId
            : (is_array($aud) && in_array($this->clientId, $aud, true));
        if (!$audMatch) {
            return null;
        }

        return $payload;
    }
}
