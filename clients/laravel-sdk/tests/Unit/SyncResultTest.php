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
}
