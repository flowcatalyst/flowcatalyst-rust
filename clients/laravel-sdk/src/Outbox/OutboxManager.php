<?php

declare(strict_types=1);

namespace FlowCatalyst\Outbox;

use FlowCatalyst\Enums\MessageType;
use FlowCatalyst\Exceptions\OutboxException;
use FlowCatalyst\Outbox\Contracts\OutboxDriver;
use FlowCatalyst\Outbox\Drivers\DatabaseDriver;
use FlowCatalyst\Outbox\DTOs\CreateAuditLogDto;
use FlowCatalyst\Outbox\DTOs\CreateDispatchJobDto;
use FlowCatalyst\Outbox\DTOs\CreateEventDto;
use FlowCatalyst\Support\TsidGenerator;

class OutboxManager
{
    public function __construct(
        private readonly OutboxDriver $driver,
        private readonly string $clientId,
    ) {}

    /**
     * Create a DatabaseDriver for a specific connection.
     *
     * Useful for writing outbox messages to a different database connection
     * than the default one configured in the service provider.
     */
    public static function driverForConnection(?string $connection, string $table = 'outbox_messages'): DatabaseDriver
    {
        return new DatabaseDriver($connection, $table);
    }

    /**
     * Create an event in the outbox.
     *
     * @return string The generated message ID (TSID)
     * @throws OutboxException
     */
    public function createEvent(CreateEventDto $event, ?OutboxDriver $driver = null): string
    {
        $this->ensureClientId();
        $activeDriver = $driver ?? $this->driver;

        $id = TsidGenerator::generate();
        $payload = json_encode($event->toPayload());
        $now = date('Y-m-d H:i:s');

        $message = $this->buildMessage(
            $id,
            MessageType::EVENT,
            $event->messageGroup,
            $payload,
            !empty($event->headers) ? $event->headers : null,
            $now,
        );

        $activeDriver->insert($message);

        return $id;
    }

    /**
     * Create a dispatch job in the outbox.
     *
     * @return string The generated message ID (TSID)
     * @throws OutboxException
     */
    public function createDispatchJob(CreateDispatchJobDto $job, ?OutboxDriver $driver = null): string
    {
        $this->ensureClientId();
        $activeDriver = $driver ?? $this->driver;

        $id = TsidGenerator::generate();
        $payload = json_encode($job->toPayload());
        $now = date('Y-m-d H:i:s');

        $message = $this->buildMessage(
            $id,
            MessageType::DISPATCH_JOB,
            $job->messageGroup,
            $payload,
            null,
            $now,
        );

        $activeDriver->insert($message);

        return $id;
    }

    /**
     * Create multiple events in the outbox (batch).
     *
     * @param CreateEventDto[] $events
     * @return string[] The generated message IDs (TSIDs)
     * @throws OutboxException
     */
    public function createEvents(array $events, ?OutboxDriver $driver = null): array
    {
        if (empty($events)) {
            return [];
        }

        $this->ensureClientId();
        $activeDriver = $driver ?? $this->driver;

        $messages = [];
        $ids = [];
        $now = date('Y-m-d H:i:s');

        foreach ($events as $event) {
            $id = TsidGenerator::generate();
            $ids[] = $id;

            $payload = json_encode($event->toPayload());

            $messages[] = $this->buildMessage(
                $id,
                MessageType::EVENT,
                $event->messageGroup,
                $payload,
                !empty($event->headers) ? $event->headers : null,
                $now,
            );
        }

        $activeDriver->insertBatch($messages);

        return $ids;
    }

    /**
     * Create multiple dispatch jobs in the outbox (batch).
     *
     * @param CreateDispatchJobDto[] $jobs
     * @return string[] The generated message IDs (TSIDs)
     * @throws OutboxException
     */
    public function createDispatchJobs(array $jobs, ?OutboxDriver $driver = null): array
    {
        if (empty($jobs)) {
            return [];
        }

        $this->ensureClientId();
        $activeDriver = $driver ?? $this->driver;

        $messages = [];
        $ids = [];
        $now = date('Y-m-d H:i:s');

        foreach ($jobs as $job) {
            $id = TsidGenerator::generate();
            $ids[] = $id;

            $payload = json_encode($job->toPayload());

            $messages[] = $this->buildMessage(
                $id,
                MessageType::DISPATCH_JOB,
                $job->messageGroup,
                $payload,
                null,
                $now,
            );
        }

        $activeDriver->insertBatch($messages);

        return $ids;
    }

    /**
     * Create an audit log in the outbox.
     *
     * @return string The generated message ID (TSID)
     * @throws OutboxException
     */
    public function createAuditLog(CreateAuditLogDto $auditLog, ?OutboxDriver $driver = null): string
    {
        $this->ensureClientId();
        $activeDriver = $driver ?? $this->driver;

        $id = TsidGenerator::generate();
        $payload = json_encode($auditLog->toPayload());
        $now = date('Y-m-d H:i:s');

        $message = $this->buildMessage(
            $id,
            MessageType::AUDIT_LOG,
            null,
            $payload,
            !empty($auditLog->headers) ? $auditLog->headers : null,
            $now,
        );

        $activeDriver->insert($message);

        return $id;
    }

    /**
     * Create multiple audit logs in the outbox (batch).
     *
     * @param CreateAuditLogDto[] $auditLogs
     * @return string[] The generated message IDs (TSIDs)
     * @throws OutboxException
     */
    public function createAuditLogs(array $auditLogs, ?OutboxDriver $driver = null): array
    {
        if (empty($auditLogs)) {
            return [];
        }

        $this->ensureClientId();
        $activeDriver = $driver ?? $this->driver;

        $messages = [];
        $ids = [];
        $now = date('Y-m-d H:i:s');

        foreach ($auditLogs as $auditLog) {
            $id = TsidGenerator::generate();
            $ids[] = $id;

            $payload = json_encode($auditLog->toPayload());

            $messages[] = $this->buildMessage(
                $id,
                MessageType::AUDIT_LOG,
                null,
                $payload,
                !empty($auditLog->headers) ? $auditLog->headers : null,
                $now,
            );
        }

        $activeDriver->insertBatch($messages);

        return $ids;
    }

    /**
     * Get the underlying driver.
     */
    public function driver(): OutboxDriver
    {
        return $this->driver;
    }

    /**
     * Build an outbox message array matching the outbox poller's schema.
     *
     * Status is integer 0 (PENDING), not a string.
     * The processor manages all other status transitions.
     */
    private function buildMessage(
        string $id,
        MessageType $type,
        ?string $messageGroup,
        string $payload,
        ?array $headers,
        string $now,
    ): array {
        return [
            'id' => $id,
            'type' => $type->value,
            'message_group' => $messageGroup,
            'payload' => $payload,
            'status' => 0,
            'created_at' => $now,
            'updated_at' => $now,
            'client_id' => $this->clientId,
            'payload_size' => strlen($payload),
            'headers' => $headers,
        ];
    }

    /**
     * Ensure client ID is configured.
     *
     * @throws OutboxException
     */
    private function ensureClientId(): void
    {
        if (empty($this->clientId)) {
            throw OutboxException::missingClientId();
        }
    }
}
