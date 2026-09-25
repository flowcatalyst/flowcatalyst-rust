<?php

declare(strict_types=1);

namespace FlowCatalyst\Outbox\Drivers;

use FlowCatalyst\Exceptions\OutboxException;
use FlowCatalyst\Outbox\Contracts\OutboxDriver;

/**
 * MongoDB driver for outbox messages.
 *
 * Requires mongodb/laravel-mongodb package.
 */
class MongoDriver implements OutboxDriver
{
    public function __construct(
        private readonly ?string $connection,
        private readonly string $collection = 'outbox_messages'
    ) {}

    /**
     * {@inheritdoc}
     */
    public function insert(array $message): void
    {
        $this->ensureMongoDbAvailable();

        try {
            $this->getCollection()->insert($this->prepareMessage($message));
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

        $this->ensureMongoDbAvailable();

        try {
            $prepared = array_map(
                fn(array $message) => $this->prepareMessage($message),
                $messages
            );

            $this->getCollection()->insert($prepared);
        } catch (\Exception $e) {
            throw OutboxException::insertFailed($e->getMessage());
        }
    }

    /**
     * Prepare a message for MongoDB insertion.
     * Field names use camelCase — the wire contract the outbox poller expects.
     */
    private function prepareMessage(array $message): array
    {
        $now = new \MongoDB\BSON\UTCDateTime(strtotime($message['created_at']) * 1000);

        return [
            // Processor-required fields (camelCase for MongoDB)
            '_id' => $message['id'],
            'type' => $message['type'],
            'messageGroup' => $message['message_group'] ?? null,
            'payload' => $message['payload'],
            'status' => $message['status'],
            'retryCount' => 0,
            'createdAt' => $now,
            'updatedAt' => $now,
            'errorMessage' => null,
            // SDK-specific fields
            'clientId' => $message['client_id'],
            'payloadSize' => $message['payload_size'],
            'headers' => $message['headers'] ?? null,
        ];
    }

    /**
     * Get the MongoDB collection.
     *
     * @return \MongoDB\Collection
     */
    private function getCollection()
    {
        $connection = $this->connection ?? config('database.default');

        return app('db')->connection($connection)->getCollection($this->collection);
    }

    /**
     * Ensure MongoDB extension and package are available.
     */
    private function ensureMongoDbAvailable(): void
    {
        if (!extension_loaded('mongodb')) {
            throw OutboxException::mongoDbNotInstalled();
        }

        if (!class_exists(\MongoDB\BSON\UTCDateTime::class)) {
            throw OutboxException::mongoDbNotInstalled();
        }
    }
}
