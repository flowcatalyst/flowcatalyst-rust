<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Webhook;

use FlowCatalyst\Webhook\WebhookValidator;
use PHPUnit\Framework\TestCase;

/**
 * Owner ruling 2026-09-25 (backlog item 11): check() returns the outcome of the same
 * check validate() performs, instead of throwing.
 */
final class WebhookVerificationTest extends TestCase
{
    public function testCheckReturnsValidForAGenuineDeliveryAndInvalidWithTheReasonOtherwise(): void
    {
        $secret = 's3cret';
        $validator = new WebhookValidator($secret);
        $timestamp = gmdate('Y-m-d\TH:i:s') . '.000Z';
        $payload = '{"a":1}';
        $signature = hash_hmac('sha256', $timestamp . $payload, $secret);

        $good = $validator->check($payload, $signature, $timestamp);
        $this->assertTrue($good->valid);
        $this->assertNull($good->reason);

        $tampered = $validator->check('{"a":2}', $signature, $timestamp);
        $this->assertFalse($tampered->valid);
        $this->assertSame('Invalid webhook signature.', $tampered->reason);

        $old = $validator->check($payload, $signature, '1000000000');
        $this->assertFalse($old->valid);
        $this->assertStringContainsString('too old', (string) $old->reason);
    }
}
