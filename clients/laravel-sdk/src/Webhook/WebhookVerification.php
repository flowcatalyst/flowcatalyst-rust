<?php

declare(strict_types=1);

namespace FlowCatalyst\Webhook;

/**
 * The outcome of {@see WebhookValidator::check()}: a returned result, so a handler
 * branches on it rather than catching. {@see WebhookValidator::validate()} is the
 * throwing form of the same check.
 */
final class WebhookVerification
{
    private function __construct(
        public readonly bool $valid,
        public readonly ?string $reason,
    ) {}

    public static function valid(): self
    {
        return new self(true, null);
    }

    public static function invalid(string $reason): self
    {
        return new self(false, $reason);
    }
}
