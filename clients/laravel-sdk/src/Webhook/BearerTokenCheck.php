<?php

declare(strict_types=1);

namespace FlowCatalyst\Webhook;

/**
 * Optional second gate LAYERED ON TOP of signature validation (never instead
 * of it — a static bearer is replayable-once-observed and binds no payload,
 * so "signature OR bearer" would downgrade security to the weaker check).
 * Platform-originated deliveries (router webhooks, scheduled-job firings,
 * dispatch deliveries) all carry `Authorization: Bearer <SA auth token>`
 * alongside the signature; when the app configures the expected token
 * (flowcatalyst.webhook_auth_token), the middleware requires BOTH to pass.
 */
final class BearerTokenCheck
{
    /**
     * Returns a human-readable error when the check fails, null when it
     * passes. A null/empty $expected means the check is not configured and
     * always passes — signature validation alone remains the boundary.
     */
    public static function check(?string $expected, ?string $authorizationHeader): ?string
    {
        if ($expected === null || $expected === '') {
            return null;
        }
        if ($authorizationHeader === null || $authorizationHeader === '') {
            return 'Missing Authorization bearer token.';
        }
        if (!str_starts_with($authorizationHeader, 'Bearer ')) {
            return 'Authorization header is not a bearer token.';
        }
        $presented = substr($authorizationHeader, strlen('Bearer '));
        if (!hash_equals($expected, $presented)) {
            return 'Invalid bearer token.';
        }
        return null;
    }
}
