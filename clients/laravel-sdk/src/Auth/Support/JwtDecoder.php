<?php

declare(strict_types=1);

namespace FlowCatalyst\Auth\Support;

/**
 * Decodes a JWT's payload with NO signature verification. Safe only for
 * tokens the platform just issued to us directly over TLS (a token exchange
 * or refresh response) — never for a token that arrived from an untrusted
 * caller (that path is {@see AccessTokenValidator}, which verifies against
 * JWKS).
 */
final class JwtDecoder
{
    /**
     * @return array<string, mixed>|null null if the JWT is malformed.
     */
    public static function decodePayload(string $jwt): ?array
    {
        $parts = explode('.', $jwt);
        if (count($parts) !== 3) {
            return null;
        }
        $payload = json_decode(base64_decode(strtr($parts[1], '-_', '+/'), true) ?: '', true);

        return is_array($payload) ? $payload : null;
    }
}
