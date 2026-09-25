<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit;

use FlowCatalyst\DTOs\Responses\SyncResult;
use PHPUnit\Framework\TestCase;

/**
 * SyncResult covers both the app-scoped syncs (which return `syncedCodes`) and
 * the application-less user sync (POST /api/principals/sync, which returns
 * `syncedEmails`).
 */
final class SyncResultTest extends TestCase
{
    public function test_reads_synced_emails_from_user_sync(): void
    {
        $r = SyncResult::fromArray([
            'created' => 2,
            'updated' => 1,
            'deleted' => 0,
            'syncedEmails' => ['a@example.com', 'b@example.com'],
        ]);

        self::assertSame(2, $r->created);
        self::assertSame(1, $r->updated);
        self::assertSame('', $r->applicationCode, 'app-less sync has no application code');
        self::assertSame(['a@example.com', 'b@example.com'], $r->syncedCodes);
    }

    public function test_prefers_synced_codes_when_present(): void
    {
        $r = SyncResult::fromArray([
            'applicationCode' => 'orders',
            'created' => 1,
            'syncedCodes' => ['orders:role'],
            'syncedEmails' => ['ignored@example.com'],
        ]);

        self::assertSame('orders', $r->applicationCode);
        self::assertSame(['orders:role'], $r->syncedCodes);
    }

    /** Owner decision 22 of 2026-09-25: a sync uses passwordHash only to create a user. */
    public function test_reads_password_hash_ignored_and_defaults_to_empty(): void
    {
        $r = SyncResult::fromArray([
            'created' => 0,
            'updated' => 1,
            'syncedEmails' => ['a@example.com'],
            'passwordHashIgnored' => ['a@example.com'],
        ]);
        self::assertSame(['a@example.com'], $r->passwordHashIgnored);

        $none = SyncResult::fromArray(['created' => 1, 'syncedEmails' => ['b@example.com']]);
        self::assertSame([], $none->passwordHashIgnored, 'omitted when empty');
    }
}
