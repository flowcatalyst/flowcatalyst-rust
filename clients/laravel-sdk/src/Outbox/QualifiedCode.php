<?php

declare(strict_types=1);

namespace FlowCatalyst\Outbox;

use InvalidArgumentException;

/**
 * Every code the platform facets on — event `type`s and dispatch-job `code`s —
 * must be a fully qualified `application:subdomain:aggregate:action` string.
 * The platform projects the first three segments out as application/subdomain/
 * aggregate facets; a bare one-word code both renders as `name:::` in the UI
 * and facets under the WRONG application (segment 1), and it denies the
 * delivery pipeline the application linkage it uses to resolve signing
 * credentials. The SDK therefore refuses to emit unqualified codes.
 */
final class QualifiedCode
{
    /** @throws InvalidArgumentException when $value is not fully qualified */
    public static function assert(string $value, string $field): void
    {
        $segments = explode(':', $value);
        $valid = count($segments) === 4;
        if ($valid) {
            foreach ($segments as $segment) {
                if (trim($segment) === '') {
                    $valid = false;
                    break;
                }
            }
        }
        if (!$valid) {
            throw new InvalidArgumentException(
                "{$field} '{$value}' must be a fully qualified code with four non-empty "
                . "colon-separated segments: application:subdomain:aggregate:action "
                . "(e.g. 'fulfil-go:fulfilment:part:create-pick')"
            );
        }
    }
}
