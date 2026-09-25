<?php

declare(strict_types=1);

namespace FlowCatalyst\Auth\Support;

/**
 * Shared RS256 JWT signature verification against JWKS, used by both
 * {@see AccessTokenValidator} (Bearer access tokens) and
 * {@see IdTokenValidator} (OIDC id_tokens issued at login/refresh).
 *
 * Only the structural/cryptographic part lives here — kid lookup and
 * `openssl_verify`. Callers apply their own claim policy (exp/nbf/iss/aud/
 * sub) to the returned payload; this class never decides whether a token is
 * *authoritative*, only whether it is *genuine*.
 *
 * Implemented without a JWT library to keep the dependency surface small and
 * avoid security-advisory churn in the JWT-library ecosystem.
 */
final class JwtSignature
{
    /**
     * Verify `$jwt`'s RS256 signature against the JWKS for `$baseUrl`.
     *
     * @return array{header: array<string, mixed>, payload: array<string, mixed>}|null
     *   null on any structural or cryptographic failure (malformed token,
     *   non-RS256 alg, unknown kid, or a signature that does not verify).
     */
    public static function verify(string $jwt, JwksCache $jwks, string $baseUrl): ?array
    {
        $parts = explode('.', $jwt);
        if (count($parts) !== 3) {
            return null;
        }
        [$header64, $payload64, $signature64] = $parts;

        $header = self::b64uJson($header64);
        $payload = self::b64uJson($payload64);
        $signature = self::b64uDecode($signature64);
        if ($header === null || $payload === null || $signature === '') {
            return null;
        }
        if (($header['alg'] ?? null) !== 'RS256') {
            return null;
        }
        $kid = $header['kid'] ?? null;
        if (!is_string($kid) || $kid === '') {
            return null;
        }

        $keys = $jwks->keys($baseUrl);
        if (!isset($keys[$kid])) {
            // JWKS may have rotated since cache fill; invalidate and retry once.
            $jwks->invalidate($baseUrl);
            $keys = $jwks->keys($baseUrl);
            if (!isset($keys[$kid])) {
                return null;
            }
        }

        $pem = self::jwkToPem($keys[$kid]);
        if ($pem === null) {
            return null;
        }

        $signingInput = "{$header64}.{$payload64}";
        $verifyResult = openssl_verify($signingInput, $signature, $pem, OPENSSL_ALGO_SHA256);
        if ($verifyResult !== 1) {
            return null;
        }

        return ['header' => $header, 'payload' => $payload];
    }

    public static function b64uDecode(string $s): string
    {
        $remainder = strlen($s) % 4;
        if ($remainder > 0) {
            $s .= str_repeat('=', 4 - $remainder);
        }
        $decoded = base64_decode(strtr($s, '-_', '+/'), true);
        return $decoded === false ? '' : $decoded;
    }

    /**
     * @return array<string, mixed>|null
     */
    public static function b64uJson(string $s): ?array
    {
        $raw = self::b64uDecode($s);
        if ($raw === '') {
            return null;
        }
        try {
            $decoded = json_decode($raw, true, 512, JSON_THROW_ON_ERROR);
        } catch (\Throwable) {
            return null;
        }
        return is_array($decoded) ? $decoded : null;
    }

    /**
     * Convert an RSA JWK (n, e) to a PEM-encoded SubjectPublicKeyInfo so we
     * can hand it to `openssl_verify`. Pure DER assembly; no external libs.
     *
     * @param array<string, mixed> $jwk
     */
    private static function jwkToPem(array $jwk): ?string
    {
        if (($jwk['kty'] ?? null) !== 'RSA' || !isset($jwk['n'], $jwk['e'])) {
            return null;
        }
        if (!is_string($jwk['n']) || !is_string($jwk['e'])) {
            return null;
        }
        $n = self::b64uDecode($jwk['n']);
        $e = self::b64uDecode($jwk['e']);
        if ($n === '' || $e === '') {
            return null;
        }

        $rsaPubKey = self::derSequence(
            self::derInteger($n) . self::derInteger($e),
        );
        // AlgorithmIdentifier OID: rsaEncryption (1.2.840.113549.1.1.1) + NULL params.
        $algorithmIdentifier = self::derSequence(
            "\x06\x09\x2a\x86\x48\x86\xf7\x0d\x01\x01\x01\x05\x00",
        );
        $bitString = self::derBitString($rsaPubKey);
        $spki = self::derSequence($algorithmIdentifier . $bitString);

        return "-----BEGIN PUBLIC KEY-----\n"
            . chunk_split(base64_encode($spki), 64, "\n")
            . "-----END PUBLIC KEY-----\n";
    }

    private static function derLength(int $length): string
    {
        if ($length < 0x80) {
            return chr($length);
        }
        $bytes = ltrim(pack('N', $length), "\x00");
        return chr(0x80 | strlen($bytes)) . $bytes;
    }

    private static function derSequence(string $contents): string
    {
        return "\x30" . self::derLength(strlen($contents)) . $contents;
    }

    private static function derInteger(string $bytes): string
    {
        // INTEGER must be twos-complement; for positive integers with MSB set
        // we have to prepend a zero byte to keep them positive.
        if ($bytes !== '' && (ord($bytes[0]) & 0x80) !== 0) {
            $bytes = "\x00" . $bytes;
        }
        return "\x02" . self::derLength(strlen($bytes)) . $bytes;
    }

    private static function derBitString(string $bytes): string
    {
        return "\x03" . self::derLength(strlen($bytes) + 1) . "\x00" . $bytes;
    }
}
