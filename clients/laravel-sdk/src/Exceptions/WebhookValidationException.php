<?php

declare(strict_types=1);

namespace FlowCatalyst\Exceptions;

class WebhookValidationException extends FlowCatalystException
{
    /**
     * Create an exception for missing signature header.
     */
    public static function missingSignature(): static
    {
        return new static('Missing X-FlowCatalyst-Signature header.', 401);
    }

    /**
     * Create an exception for missing timestamp header.
     */
    public static function missingTimestamp(): static
    {
        return new static('Missing X-FlowCatalyst-Timestamp header.', 401);
    }

    /**
     * Create an exception for invalid signature.
     */
    public static function invalidSignature(): static
    {
        return new static('Invalid webhook signature.', 401);
    }

    /**
     * Create an exception for expired timestamp.
     */
    public static function timestampExpired(int $tolerance): static
    {
        return new static("Webhook timestamp is too old. Tolerance is {$tolerance} seconds.", 401);
    }

    /**
     * Create an exception for future timestamp.
     */
    public static function timestampInFuture(): static
    {
        return new static('Webhook timestamp is in the future.', 401);
    }

    /**
     * Create an exception for an unparseable timestamp.
     */
    public static function invalidTimestamp(): static
    {
        return new static('Webhook timestamp is not a valid ISO8601 or Unix-seconds value.', 401);
    }

    /**
     * Create an exception for missing signing secret.
     */
    public static function missingSigningSecret(): static
    {
        return new static('Signing secret is not configured. Set FLOWCATALYST_SIGNING_SECRET in your environment.', 500);
    }
}
