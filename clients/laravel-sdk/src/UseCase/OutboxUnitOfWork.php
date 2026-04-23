<?php

declare(strict_types=1);

namespace FlowCatalyst\UseCase;

use FlowCatalyst\Outbox\DTOs\CreateAuditLogDto;
use FlowCatalyst\Outbox\DTOs\CreateEventDto;
use FlowCatalyst\Outbox\OutboxManager;

/**
 * UnitOfWork that dispatches events through the existing `OutboxManager`.
 *
 * On `commit()`, the `DomainEvent` is mapped into a `CreateEventDto`
 * (source / subject / correlationId / messageGroup / deduplicationId plus
 * principalId / executionId / aggregateType as contextData) and inserted
 * into the outbox table. The fc-outbox-processor forwards it to the
 * platform.
 *
 * For atomic persistence of your entity + the outbox write, wrap the
 * whole `commit()` call in `DB::transaction(fn () => ...)` using a
 * `DatabaseDriver` on the same connection — both inserts end up in one tx.
 */
final class OutboxUnitOfWork implements UnitOfWork
{
    public function __construct(
        private readonly OutboxManager $outboxManager,
        private readonly bool $auditEnabled = false,
        private readonly string $fallbackPrincipalId = 'system',
    ) {}

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

        return CreateAuditLogDto::create(
            entityType: $entityType,
            entityId:   $entityId,
            operation:  $operation,
        )
            ->withOperationData($operationData)
            ->withPrincipalId($event->principalId() !== '' ? $event->principalId() : $this->fallbackPrincipalId)
            ->withCorrelationId($event->correlationId())
            ->withSource($event->source())
            ->withPerformedAt($event->time());
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
