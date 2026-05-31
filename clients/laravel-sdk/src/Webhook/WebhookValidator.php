<?php

declare(strict_types=1);

namespace FlowCatalyst\Webhook;

use FlowCatalyst\Exceptions\WebhookValidationException;
use Illuminate\Http\Request;

/**
 * Validates incoming webhook signatures from FlowCatalyst using HMAC-SHA256.
 */
class WebhookValidator
{
    public function __construct(
        private readonly string $signingSecret
    ) {}

    /**
     * Validate a webhook signature.
     *
     * @param string $payload Raw request body
     * @param string $signature Value of X-FlowCatalyst-Signature header
     * @param string $timestamp Value of X-FlowCatalyst-Timestamp header
     * @param int $tolerance Max age in seconds (default 300 = 5 minutes)
     * @throws WebhookValidationException
     */
    public function validate(
        string $payload,
        string $signature,
        string $timestamp,
        int $tolerance = 300
    ): bool {
        // Validate timestamp
        $this->validateTimestamp($timestamp, $tolerance);

        // Compute expected signature
        $message = $timestamp . $payload;
        $expectedSignature = hash_hmac('sha256', $message, $this->signingSecret);

        // Constant-time comparison to prevent timing attacks
        if (!hash_equals($expectedSignature, $signature)) {
            throw WebhookValidationException::invalidSignature();
        }

        return true;
    }

    /**
     * Validate a webhook from a Laravel Request.
     *
     * @param Request $request The incoming request
     * @param int $tolerance Max age in seconds (default 300 = 5 minutes)
     * @throws WebhookValidationException
     */
    public function validateRequest(Request $request, int $tolerance = 300): bool
    {
        $signature = $request->header('X-FlowCatalyst-Signature');
        $timestamp = $request->header('X-FlowCatalyst-Timestamp');

        if (empty($signature)) {
            throw WebhookValidationException::missingSignature();
        }

        if (empty($timestamp)) {
            throw WebhookValidationException::missingTimestamp();
        }

        return $this->validate(
            payload: $request->getContent(),
            signature: $signature,
            timestamp: $timestamp,
            tolerance: $tolerance
        );
    }

    /**
     * Validate the timestamp is within tolerance.
     *
     * @throws WebhookValidationException
     */
    private function validateTimestamp(string $timestamp, int $tolerance): void
    {
        $webhookTime = $this->parseTimestamp($timestamp);
        if ($webhookTime === null) {
            throw WebhookValidationException::invalidTimestamp();
        }
        $currentTime = time();

        // Check if timestamp is too old
        if ($webhookTime < ($currentTime - $tolerance)) {
            throw WebhookValidationException::timestampExpired($tolerance);
        }

        // Check if timestamp is in the future (with 60 second grace period)
        if ($webhookTime > ($currentTime + 60)) {
            throw WebhookValidationException::timestampInFuture();
        }
    }

    /**
     * Parse the X-FlowCatalyst-Timestamp header value into Unix seconds.
     *
     * The FlowCatalyst router emits an ISO8601 timestamp with millisecond
     * precision (e.g. 2026-05-24T08:30:00.123Z). For backward compatibility
     * we also accept a bare Unix-seconds integer. Returns null when the
     * value is unparseable.
     *
     * Note: the HMAC is computed over the raw header string (timestamp .
     * payload), so this parsing only affects the replay-window check — it
     * never changes signature verification.
     */
    private function parseTimestamp(string $timestamp): ?int
    {
        // Backward-compat: a bare Unix-seconds integer.
        if (ctype_digit($timestamp)) {
            return (int) $timestamp;
        }

        // ISO8601 (handles the trailing Z and fractional seconds).
        try {
            return (new \DateTimeImmutable($timestamp))->getTimestamp();
        } catch (\Exception) {
            return null;
        }
    }

    /**
     * Create a validator from configuration.
     *
     * @throws WebhookValidationException
     */
    public static function fromConfig(): self
    {
        $secret = config('flowcatalyst.signing_secret');

        if (empty($secret)) {
            throw WebhookValidationException::missingSigningSecret();
        }

        return new self($secret);
    }
}
