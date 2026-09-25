<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Webhook;

use FlowCatalyst\Exceptions\WebhookValidationException;
use FlowCatalyst\Webhook\WebhookValidator;
use PHPUnit\Framework\TestCase;

/**
 * Pins the signature contract shared with the platform: the Go scheduled-job
 * dispatcher (and the router's webhook mediator) sign HMAC-SHA256 over
 * `timestamp . rawBody`, hex-encoded, with a millisecond-precision ISO8601
 * UTC timestamp. The validator must accept exactly that shape — the Go side
 * pins the same bytes in TestDispatcherTick_SignsFiringWithApplicationSecret.
 */
final class WebhookValidatorCompatTest extends TestCase
{
    private const SECRET = 'sjdsp-signing-secret-1';

    /** Sign exactly as the Go dispatcher does (signFiring). */
    private function goSign(string $timestamp, string $body): string
    {
        return hash_hmac('sha256', $timestamp . $body, self::SECRET);
    }

    /** Millisecond-ISO8601 UTC, the Go `2006-01-02T15:04:05.000Z` format. */
    private function goTimestamp(int $unixSeconds): string
    {
        return gmdate('Y-m-d\TH:i:s', $unixSeconds) . '.000Z';
    }

    public function test_accepts_go_format_signature(): void
    {
        $body = '{"jobId":"sjb_1","jobCode":"nightly-report","instanceId":"sji_1",'
            . '"firedAt":"2026-08-07T00:00:00Z","triggerKind":"CRON",'
            . '"tracksCompletion":false,"concurrent":false}';
        $ts = $this->goTimestamp(time());

        $validator = new WebhookValidator(self::SECRET);
        $this->assertTrue($validator->validate($body, $this->goSign($ts, $body), $ts));
    }

    public function test_rejects_tampered_body(): void
    {
        $ts = $this->goTimestamp(time());
        $validator = new WebhookValidator(self::SECRET);

        $this->expectException(WebhookValidationException::class);
        $validator->validate('{"tampered":true}', $this->goSign($ts, '{"original":true}'), $ts);
    }

    public function test_rejects_replayed_old_timestamp(): void
    {
        $ts = $this->goTimestamp(time() - 600); // > 5-minute tolerance
        $body = '{"replayed":true}';
        $validator = new WebhookValidator(self::SECRET);

        $this->expectException(WebhookValidationException::class);
        $validator->validate($body, $this->goSign($ts, $body), $ts);
    }

    public function test_rejects_wrong_secret(): void
    {
        $ts = $this->goTimestamp(time());
        $body = '{"k":"v"}';
        $validator = new WebhookValidator('a-different-secret');

        $this->expectException(WebhookValidationException::class);
        $validator->validate($body, $this->goSign($ts, $body), $ts);
    }
}
