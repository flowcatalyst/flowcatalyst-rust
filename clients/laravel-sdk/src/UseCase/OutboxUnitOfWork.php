<?php

declare(strict_types=1);

namespace FlowCatalyst\UseCase;

use FlowCatalyst\Outbox\DTOs\CreateAuditLogDto;
use FlowCatalyst\Outbox\DTOs\CreateEventDto;
use FlowCatalyst\Outbox\OutboxManager;
use Illuminate\Support\Facades\DB;

/**
 * UnitOfWork that dispatches events through the existing `OutboxManager`.
 *
 * On `commit()`, the `DomainEvent` is mapped into a `CreateEventDto`
 * (source / subject / correlationId / messageGroup / deduplicationId plus
 * principalId / executionId / aggregateType as contextData) and inserted
 * into the outbox table. The outbox poller forwards it to the
 * platform.
 *
 * Authoring goes through the use-case envelope ({@link Operation} +
 * {@link Runner::run()}): `run` calls {@link transaction()} to open one
 * transaction on `$connection`, then applies the {@link Plan} (aggregate write
 * via its Repo + the outbox event) inside it — so the unit of work owns
 * atomicity. The raw `commit()` family below remains for the lower-level path
 * and for back-compat.
 */
final class OutboxUnitOfWork implements UnitOfWork
{
    public function __construct(
        private readonly OutboxManager $outboxManager,
        private readonly bool $auditEnabled = false,
        private readonly string $fallbackPrincipalId = 'system',
        /**
         * The connection the owned transaction runs on. Must match the outbox
         * driver's connection so the event row joins the transaction; null uses
         * the default connection (the common case).
         */
        private readonly ?string $connection = null,
        /**
         * The FlowCatalyst application + client codes these operations belong to.
         * The platform is client-centric: audit logs carry both (resolved to
         * application_id / client_id at ingest); events carry the client code
         * (the application is derived from the event-type prefix). Null = omit.
         */
        private readonly ?string $applicationCode = null,
        private readonly ?string $clientCode = null,
    ) {}

    /**
     * Open one transaction on the configured connection, commit on normal
     * return, roll back (and rethrow) on throw — Laravel's `DB::transaction`
     * contract. The outbox driver writes via `DB::connection($connection)`, so
     * its insert joins this transaction.
     */
    public function transaction(callable $callback): mixed
    {
        return DB::connection($this->connection)->transaction($callback);
    }

    public function commit(
        DomainEvent $event,
        mixed $command,
        ?callable $persist = null,
    ): Result {
        return $this->doCommit($event, $command, $persist);
    }

    public function commitAggregate(
        Aggregate $aggregate,
        DomainEvent $event,
        mixed $command,
        ?callable $persist = null,
    ): Result {
        // The aggregate arg is for API parity with the platform UnitOfWork —
        // persistence is the caller's responsibility via $persist.
        return $this->doCommit($event, $command, $persist);
    }

    public function commitDelete(
        Aggregate $aggregate,
        DomainEvent $event,
        mixed $command,
        ?callable $persist = null,
    ): Result {
        return $this->doCommit($event, $command, $persist);
    }

    public function emitEvent(DomainEvent $event, mixed $command): Result
    {
        return $this->doCommit($event, $command, null);
    }

    /**
     * @template T of DomainEvent
     * @param T $event
     * @param callable():void|null $persist
     * @return Result<T>
     */
    private function doCommit(DomainEvent $event, mixed $command, ?callable $persist): Result
    {
        try {
            if ($persist !== null) {
                $persist();
            }

            $this->outboxManager->createEvent($this->toEventDto($event));

            if ($this->auditEnabled) {
                $this->outboxManager->createAuditLog($this->toAuditDto($event, $command));
            }

            return Result::success(ResultSuccessToken::internal(), $event);
        } catch (\Throwable $err) {
            return Result::failure(UseCaseError::infrastructure(
                'COMMIT_FAILED',
                $err->getMessage(),
                ['cause' => $err::class],
            ));
        }
    }

    private function toEventDto(DomainEvent $event): CreateEventDto
    {
        $data = $this->parseData($event->toDataJson());

        $dto = CreateEventDto::create($event->eventType(), $data)
            ->withSource($event->source())
            ->withSubject($event->subject())
            ->withCorrelationId($event->correlationId())
            ->withMessageGroup($event->messageGroup())
            ->withDeduplicationId("{$event->eventType()}-{$event->eventId()}")
            ->withContextData([
                ['key' => 'principalId',   'value' => $event->principalId()],
                ['key' => 'executionId',   'value' => $event->executionId()],
                ['key' => 'aggregateType', 'value' => DomainEventHelpers::extractAggregateType($event->subject())],
            ]);

        if ($event->causationId() !== null) {
            $dto = $dto->withCausationId($event->causationId());
        }

        // Client linkage (platform resolves the code → client_id at ingest).
        // The application is derived from the event-type prefix on the platform.
        if ($this->clientCode !== null && $this->clientCode !== '') {
            $dto = $dto->withClientCode($this->clientCode);
        }

        return $dto;
    }

    private function toAuditDto(DomainEvent $event, mixed $command): CreateAuditLogDto
    {
        $entityId   = DomainEventHelpers::extractEntityId($event->subject()) ?? '';
        $entityType = DomainEventHelpers::extractAggregateType($event->subject());
        $parts      = explode(':', $event->eventType());
        $operation  = $parts[count($parts) - 1] ?? 'unknown';

        $operationData = match (true) {
            is_array($command)  => $command,
            is_object($command) => (array) $command,
            default             => ['command' => $command],
        };

        $dto = CreateAuditLogDto::create(
            entityType: $entityType,
            entityId:   $entityId,
            operation:  $operation,
        )
            ->withOperationData(
                $operationData,
                $command instanceof AuditMasked ? $command->auditMaskedFields() : [],
            )
            ->withPrincipalId($event->principalId() !== '' ? $event->principalId() : $this->fallbackPrincipalId)
            ->withCorrelationId($event->correlationId())
            ->withSource($event->source())
            ->withPerformedAt($event->time());

        // Application + client linkage (the platform is client-centric and
        // resolves these codes → application_id / client_id at ingest).
        if ($this->applicationCode !== null && $this->applicationCode !== '') {
            $dto = $dto->withApplicationCode($this->applicationCode);
        }
        if ($this->clientCode !== null && $this->clientCode !== '') {
            $dto = $dto->withClientCode($this->clientCode);
        }

        return $dto;
    }

    /**
     * @return array<string, mixed>
     */
    private function parseData(string $json): array
    {
        try {
            $decoded = json_decode($json, associative: true, flags: JSON_THROW_ON_ERROR);
        } catch (\JsonException) {
            return [];
        }
        return is_array($decoded) ? $decoded : [];
    }
}
