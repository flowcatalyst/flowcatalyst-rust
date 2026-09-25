<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Sync;

use FlowCatalyst\DTOs\Requests\SyncPrincipalEntry;
use FlowCatalyst\Sync\PrincipalDefinition;
use PHPUnit\Framework\TestCase;

/**
 * Locks the principal-sync wire contract for the optional `passwordHash`
 * field: it carries an already-hashed credential verbatim so a migrated user
 * keeps their password, and is OMITTED entirely when null so a roles-only sync
 * never wipes an existing password on the platform.
 */
final class PrincipalPasswordHashSyncTest extends TestCase
{
    public function test_password_hash_is_emitted_when_set(): void
    {
        $hash = '$2y$10$abcdefghijklmnopqrstuv';
        $def = PrincipalDefinition::make('john@example.com', 'John Doe')
            ->withRoles(['admin'])
            ->withPasswordHash($hash);

        $arr = $def->toArray();
        self::assertSame($hash, $arr['passwordHash']);
        self::assertSame(['admin'], $arr['roles']);
    }

    public function test_password_hash_is_omitted_when_null(): void
    {
        $arr = PrincipalDefinition::make('jane@example.com', 'Jane Doe')->toArray();
        self::assertArrayNotHasKey('passwordHash', $arr);
    }

    public function test_builders_preserve_password_hash(): void
    {
        $hash = '$2y$10$preserveme';
        $def = PrincipalDefinition::make('a@example.com', 'A')->withPasswordHash($hash);

        self::assertSame($hash, $def->withRoles(['x'])->passwordHash, 'withRoles keeps the hash');
        self::assertSame($hash, $def->inactive()->passwordHash, 'inactive keeps the hash');
    }

    public function test_from_array_round_trips_password_hash(): void
    {
        $hash = '$argon2i$v=19$m=65536,t=4,p=1$c2FsdA$aGFzaA';
        $def = PrincipalDefinition::fromArray([
            'email' => 'b@example.com',
            'name' => 'B',
            'passwordHash' => $hash,
        ]);
        self::assertSame($hash, $def->passwordHash);
        self::assertSame($hash, $def->toArray()['passwordHash']);
    }

    public function test_sync_entry_emits_and_omits_password_hash(): void
    {
        $hash = '$2y$10$entryhash';
        $withHash = new SyncPrincipalEntry('c@example.com', 'C', ['admin'], true, $hash);
        self::assertSame($hash, $withHash->toArray()['passwordHash']);

        $withoutHash = new SyncPrincipalEntry('d@example.com', 'D', ['admin'], true);
        self::assertArrayNotHasKey('passwordHash', $withoutHash->toArray());
    }
}
