<?php

declare(strict_types=1);

namespace FlowCatalyst\Outbox\Drivers;

use FlowCatalyst\Exceptions\OutboxException;
use FlowCatalyst\Outbox\Contracts\OutboxDriver;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Log;

/**
 * Database driver for MySQL 8.0+ and PostgreSQL 12+.
 *
 * Transactional safety
 * --------------------
 * Outbox writes are only atomic with your business writes if both happen
 * inside the same database transaction on the same connection. This driver
 * uses `DB::connection($name)`, which inherits whatever transaction stack
 * the connection currently holds — so wrapping your call in
 * `DB::transaction(fn () => ...)` makes the outbox row commit (or roll back)
 * with your business changes.
 *
 * The `$strictTransactions` flag controls what happens when no transaction
 * is active:
 *
 *  - `true`  — throw `OutboxException::noActiveTransaction()`. Use this in
 *              new code that should always be transactionally framed.
 *  - `false` — log a warning via `Log::warning(...)` and let the write
 *              proceed unfenced. This is the default for backwards
 *              compatibility; flip it on per environment when callers are
 *              ready.
 */
class DatabaseDriver implements OutboxDriver
{
    public function __construct(
        private readonly ?string $connection,
        private readonly string $table = 'outbox_messages',
        private readonly bool $strictTransactions = false,
    ) {}

    /**
     * {@inheritdoc}
     */
    public function insert(array $message): void
    {
        $this->assertTransactionalContext();

        try {
            $this->getConnection()->table($this->table)->insert(
                $this->prepareMessage($message)
            );
        } catch (\Exception $e) {
            throw OutboxException::insertFailed($e->getMessage());
        }
    }

    /**
     * {@inheritdoc}
     */
    public function insertBatch(array $messages): void
    {
        if (empty($messages)) {
            return;
        }

        $this->assertTransactionalContext();

        try {
            $prepared = array_map(
                fn(array $message) => $this->prepareMessage($message),
                $messages
            );

            $this->getConnection()->table($this->table)->insert($prepared);
        } catch (\Exception $e) {
            throw OutboxException::insertFailed($e->getMessage());
        }
    }

    /**
     * Refuse (strict mode) or warn (default) when the outbox is written
     * outside of an active database transaction on this connection. See
     * the class-level doc for the rationale.
     */
    private function assertTransactionalContext(): void
    {
        if ($this->getConnection()->transactionLevel() > 0) {
            return;
        }

        if ($this->strictTransactions) {
            throw OutboxException::noActiveTransaction($this->connection);
        }

        Log::warning(
            'FlowCatalyst outbox write outside of DB::transaction(...). '
            . 'Business writes and outbox writes will NOT commit atomically — '
            . 'enable flowcatalyst.outbox.strict_transactions to make this an error.',
            ['connection' => $this->connection ?? '(default)'],
        );
    }

    /**
     * Prepare a message for database insertion.
     * Column layout matches the outbox-processor schema.
     */
    private function prepareMessage(array $message): array
    {
        return [
            // Processor-required columns
            'id' => $message['id'],
            'type' => $message['type'],
            'message_group' => $message['message_group'] ?? null,
            'payload' => $message['payload'],
            'status' => $message['status'],
            'created_at' => $message['created_at'],
            'updated_at' => $message['updated_at'],
            // SDK-specific columns
            'client_id' => $message['client_id'],
            'payload_size' => $message['payload_size'],
            'headers' => isset($message['headers']) ? json_encode($message['headers']) : null,
        ];
    }

    /**
     * Get the database connection.
     */
    private function getConnection(): \Illuminate\Database\ConnectionInterface
    {
        return DB::connection($this->connection);
    }
}
