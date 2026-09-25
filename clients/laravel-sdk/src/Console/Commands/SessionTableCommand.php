<?php

declare(strict_types=1);

namespace FlowCatalyst\Console\Commands;

use Illuminate\Console\Command;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Support\Facades\Schema;

/**
 * Adjust `sessions.user_id` for FlowCatalyst's string principal ids.
 *
 * You only need this when BOTH:
 *   - SESSION_DRIVER=database, and
 *   - a FlowCatalyst guard (`fc-session` / `fc-token`) is the authenticated
 *     guard — i.e. the session user is the token principal (`prn_…`), not a
 *     local Eloquent user.
 *
 * Laravel's default `sessions.user_id` is a bigint, so Laravel writing the
 * string principal id fails with `SQLSTATE[22P02]`. This command widens it to a
 * string (and `--rollback` reverts it). Apps using a local login (bigint user),
 * or file/redis/cookie sessions, never need it.
 *
 * It is idempotent and self-contained (it does NOT touch the migrations table),
 * so it's safe to re-run.
 */
class SessionTableCommand extends Command
{
    protected $signature = 'flowcatalyst:session-table
        {--rollback : Revert sessions.user_id back to Laravel\'s default bigint}';

    protected $description = "Widen sessions.user_id to a string (or --rollback) for the FlowCatalyst guard + database sessions";

    public function handle(): int
    {
        if (! Schema::hasTable('sessions') || ! Schema::hasColumn('sessions', 'user_id')) {
            $this->warn('No `sessions` table with a `user_id` column — nothing to do.');
            $this->line('  (This is only needed with SESSION_DRIVER=database.)');
            return self::SUCCESS;
        }

        return $this->option('rollback') ? $this->rollback() : $this->apply();
    }

    private function apply(): int
    {
        if ($this->userIdIsString()) {
            $this->info('sessions.user_id is already a string — nothing to do.');
            return self::SUCCESS;
        }

        Schema::table('sessions', function (Blueprint $table) {
            $table->string('user_id')->nullable()->change();
        });

        $this->info('✓ Widened sessions.user_id to a string — the FlowCatalyst guard\'s principal ids (prn_…) now fit.');
        return self::SUCCESS;
    }

    private function rollback(): int
    {
        if (! $this->userIdIsString()) {
            $this->info('sessions.user_id is already a bigint — nothing to do.');
            return self::SUCCESS;
        }

        // varchar → bigint can't be cast automatically (and string principal ids
        // wouldn't fit), so drop + re-add the column. Sessions are ephemeral, so
        // losing user_id just logs people out on their next request.
        Schema::table('sessions', function (Blueprint $table) {
            $table->dropColumn('user_id');
        });
        Schema::table('sessions', function (Blueprint $table) {
            $table->foreignId('user_id')->nullable()->index();
        });

        $this->info('✓ Reverted sessions.user_id to Laravel\'s default bigint.');
        return self::SUCCESS;
    }

    /**
     * Is `sessions.user_id` currently a string-ish column (varchar/text)?
     * Cross-DB via the schema introspection added in Laravel 11+.
     */
    private function userIdIsString(): bool
    {
        foreach (Schema::getColumns('sessions') as $column) {
            if (($column['name'] ?? null) === 'user_id') {
                $type = strtolower((string) ($column['type_name'] ?? $column['type'] ?? ''));
                return str_contains($type, 'char') || str_contains($type, 'text') || $type === 'string';
            }
        }

        return false;
    }
}
