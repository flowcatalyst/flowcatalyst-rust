<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\UseCase;

use FlowCatalyst\UseCase\Aggregate;
use FlowCatalyst\UseCase\BaseDomainEvent;
use FlowCatalyst\UseCase\Command;
use FlowCatalyst\UseCase\DomainEvent;
use FlowCatalyst\UseCase\DomainEventHelpers;
use FlowCatalyst\UseCase\ExecutionContext;
use FlowCatalyst\UseCase\Operation;
use FlowCatalyst\UseCase\Plan;
use FlowCatalyst\UseCase\Repo;
use FlowCatalyst\UseCase\Result;
use FlowCatalyst\UseCase\ResultSuccessToken;
use FlowCatalyst\UseCase\Runner;
use FlowCatalyst\UseCase\UnitOfWork;
use FlowCatalyst\UseCase\UseCaseError;
use PHPUnit\Framework\TestCase;

/**
 * Exercises the use-case envelope (Operation + Plan + Runner) end-to-end
 * against an in-memory UnitOfWork. The fake's transaction() reproduces the
 * Laravel DB::transaction contract exactly — commit deferred writes on a normal
 * return, discard them (rollback) on a throw — so the Runner's real
 * throw-to-rollback logic is what is under test.
 */
final class EnvelopeTest extends TestCase
{
    public function test_happy_path_persists_aggregate_and_commits_event_atomically(): void
    {
        $uow = new FakeUnitOfWork();
        $repo = new FakeRepo($uow);
        $log = new CallLog();

        $result = Runner::run($uow, new PlaceOrder($repo, $log), new PlaceOrderCommand(1500), self::ctx());

        self::assertTrue($result->isSuccess(), 'expected success');
        self::assertSame(['validate', 'authorize', 'execute'], $log->calls, 'phases run in order');
        self::assertCount(1, $repo->saved, 'aggregate persisted');
        self::assertSame('ord_envelope_1', $repo->saved[0]->id());
        self::assertCount(1, $uow->committedEvents, 'exactly one event committed');
        self::assertSame('shop:orders:order:placed', $uow->committedEvents[0]->eventType());
    }

    public function test_validation_failure_short_circuits_before_authorize_and_writes_nothing(): void
    {
        $uow = new FakeUnitOfWork();
        $repo = new FakeRepo($uow);
        $log = new CallLog();

        $result = Runner::run($uow, new PlaceOrder($repo, $log), new PlaceOrderCommand(0), self::ctx());

        self::assertTrue($result->isFailure());
        self::assertSame('TOTAL_INVALID', $result->error()->code);
        self::assertSame(['validate'], $log->calls, 'authorize/execute must not run');
        self::assertCount(0, $uow->committedEvents);
        self::assertCount(0, $repo->saved);
    }

    public function test_authorization_failure_short_circuits_before_execute(): void
    {
        $uow = new FakeUnitOfWork();
        $repo = new FakeRepo($uow);
        $log = new CallLog();

        // principalId '' fails the authorize check.
        $result = Runner::run($uow, new PlaceOrder($repo, $log), new PlaceOrderCommand(1500), ExecutionContext::create(''));

        self::assertTrue($result->isFailure());
        self::assertSame('NO_PRINCIPAL', $result->error()->code);
        self::assertSame(['validate', 'authorize'], $log->calls, 'execute must not run');
        self::assertCount(0, $uow->committedEvents);
    }

    public function test_execute_returning_an_error_fails_without_touching_the_outbox(): void
    {
        $uow = new FakeUnitOfWork();

        $op = new class implements Operation {
            public function validate(Command $command): ?UseCaseError { return null; }
            public function authorize(Command $command, ExecutionContext $context): ?UseCaseError { return null; }
            public function execute(Command $command, ExecutionContext $context): Plan|UseCaseError
            {
                return UseCaseError::notFound('ORDER_NOT_FOUND', 'no such order');
            }
        };

        $result = Runner::run($uow, $op, new PlaceOrderCommand(1), self::ctx());

        self::assertTrue($result->isFailure());
        self::assertSame('ORDER_NOT_FOUND', $result->error()->code);
        self::assertCount(0, $uow->committedEvents);
    }

    public function test_plan_emit_commits_the_event_with_no_aggregate_persist(): void
    {
        $uow = new FakeUnitOfWork();

        $op = new class implements Operation {
            public function validate(Command $command): ?UseCaseError { return null; }
            public function authorize(Command $command, ExecutionContext $context): ?UseCaseError { return null; }
            public function execute(Command $command, ExecutionContext $context): Plan|UseCaseError
            {
                return Plan::emit(new OrderPlaced($context, 'ord_emit_1', 1));
            }
        };

        $result = Runner::run($uow, $op, new PlaceOrderCommand(1), self::ctx());

        self::assertTrue($result->isSuccess());
        self::assertCount(1, $uow->committedEvents);
    }

    public function test_failed_event_commit_rolls_back_the_aggregate_persist(): void
    {
        $uow = new FakeUnitOfWork();
        $uow->failCommit = true; // the outbox event write fails inside the tx
        $repo = new FakeRepo($uow);

        $result = Runner::run($uow, new PlaceOrder($repo, new CallLog()), new PlaceOrderCommand(1500), self::ctx());

        self::assertTrue($result->isFailure(), 'the write must fail');
        self::assertCount(0, $uow->committedEvents, 'no event committed');
        self::assertCount(0, $repo->saved, 'the aggregate persist must roll back with the event');
    }

    private static function ctx(): ExecutionContext
    {
        return ExecutionContext::create('prn_test');
    }
}

// ─── Fixtures ────────────────────────────────────────────────────────────────

final class PlaceOrderCommand implements Command
{
    public function __construct(public readonly int $totalCents) {}
}

final class Order implements Aggregate
{
    public function __construct(private readonly string $id, public readonly int $totalCents) {}

    public function id(): string
    {
        return $this->id;
    }
}

/**
 * @extends BaseDomainEvent<array{orderId: string, totalCents: int}>
 */
final class OrderPlaced extends BaseDomainEvent
{
    public function __construct(ExecutionContext $ctx, string $orderId, int $totalCents)
    {
        parent::__construct(
            [
                'eventType' => 'shop:orders:order:placed',
                'specVersion' => '1.0',
                'source' => 'shop:orders',
                'subject' => DomainEventHelpers::subject('orders', 'order', $orderId),
                'messageGroup' => DomainEventHelpers::messageGroup('orders', 'order', $orderId),
            ],
            $ctx,
            ['orderId' => $orderId, 'totalCents' => $totalCents],
        );
    }
}

/**
 * @implements Operation<PlaceOrderCommand, OrderPlaced>
 */
final class PlaceOrder implements Operation
{
    public function __construct(private readonly Repo $repo, private readonly CallLog $log) {}

    public function validate(Command $command): ?UseCaseError
    {
        $this->log->calls[] = 'validate';
        /** @var PlaceOrderCommand $command */
        return $command->totalCents > 0
            ? null
            : UseCaseError::validation('TOTAL_INVALID', 'totalCents must be positive');
    }

    public function authorize(Command $command, ExecutionContext $context): ?UseCaseError
    {
        $this->log->calls[] = 'authorize';
        return $context->principalId !== ''
            ? null
            : UseCaseError::authorization('NO_PRINCIPAL', 'no acting principal');
    }

    public function execute(Command $command, ExecutionContext $context): Plan|UseCaseError
    {
        $this->log->calls[] = 'execute';
        /** @var PlaceOrderCommand $command */
        $order = new Order('ord_envelope_1', $command->totalCents);
        $event = new OrderPlaced($context, $order->id(), $command->totalCents);

        return Plan::save($order, $this->repo, $event);
    }
}

final class CallLog
{
    /** @var list<string> */
    public array $calls = [];
}

final class FakeRepo implements Repo
{
    /** @var list<Aggregate> */
    public array $saved = [];

    public function __construct(private readonly FakeUnitOfWork $uow) {}

    public function persist(Aggregate $aggregate): void
    {
        // Defer to the active transaction so a rollback discards it.
        $this->uow->queue(function () use ($aggregate): void {
            $this->saved[] = $aggregate;
        });
    }

    public function delete(Aggregate $aggregate): void
    {
        $this->uow->queue(function () use ($aggregate): void {
            $this->saved = array_values(array_filter(
                $this->saved,
                fn (Aggregate $a): bool => $a->id() !== $aggregate->id(),
            ));
        });
    }
}

/**
 * In-memory UnitOfWork whose transaction() models DB::transaction exactly:
 * deferred writes (events + repo writes) flush on a normal return and are
 * discarded on a throw. Success can be minted because ResultSuccessToken's
 * accessor is intentionally reachable (the seal is convention-level in PHP).
 */
final class FakeUnitOfWork implements UnitOfWork
{
    /** @var list<DomainEvent> */
    public array $committedEvents = [];
    public bool $failCommit = false;

    /** @var list<callable():void> */
    private array $ops = [];
    private bool $inTransaction = false;

    public function transaction(callable $callback): mixed
    {
        $this->inTransaction = true;
        $this->ops = [];
        try {
            $result = $callback();
            foreach ($this->ops as $op) {
                $op(); // commit
            }
            return $result;
        } catch (\Throwable $err) {
            throw $err; // rollback: ops are discarded below
        } finally {
            $this->inTransaction = false;
            $this->ops = [];
        }
    }

    public function commit(DomainEvent $event, mixed $command, ?callable $persist = null): Result
    {
        try {
            if ($persist !== null) {
                $persist();
            }
            if ($this->failCommit) {
                throw new \RuntimeException('simulated outbox write failure');
            }
            $this->queue(function () use ($event): void {
                $this->committedEvents[] = $event;
            });
            return Result::success(ResultSuccessToken::internal(), $event);
        } catch (\Throwable $err) {
            return Result::failure(UseCaseError::infrastructure('COMMIT_FAILED', $err->getMessage()));
        }
    }

    public function commitAggregate(Aggregate $aggregate, DomainEvent $event, mixed $command, ?callable $persist = null): Result
    {
        return $this->commit($event, $command, $persist);
    }

    public function commitDelete(Aggregate $aggregate, DomainEvent $event, mixed $command, ?callable $persist = null): Result
    {
        return $this->commit($event, $command, $persist);
    }

    public function emitEvent(DomainEvent $event, mixed $command): Result
    {
        return $this->commit($event, $command, null);
    }

    /** @param callable():void $op */
    public function queue(callable $op): void
    {
        if ($this->inTransaction) {
            $this->ops[] = $op;
        } else {
            $op();
        }
    }
}
