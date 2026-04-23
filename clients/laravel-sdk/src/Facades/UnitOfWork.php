<?php

declare(strict_types=1);

namespace FlowCatalyst\Facades;

use FlowCatalyst\UseCase\Aggregate;
use FlowCatalyst\UseCase\DomainEvent;
use FlowCatalyst\UseCase\Result;
use FlowCatalyst\UseCase\UnitOfWork as UnitOfWorkContract;
use Illuminate\Support\Facades\Facade;

/**
 * @method static Result commit(DomainEvent $event, mixed $command, ?callable $persist = null)
 * @method static Result commitAggregate(Aggregate $aggregate, DomainEvent $event, mixed $command, ?callable $persist = null)
 * @method static Result commitDelete(Aggregate $aggregate, DomainEvent $event, mixed $command, ?callable $persist = null)
 * @method static Result emitEvent(DomainEvent $event, mixed $command)
 *
 * @see \FlowCatalyst\UseCase\UnitOfWork
 */
class UnitOfWork extends Facade
{
    protected static function getFacadeAccessor(): string
    {
        return UnitOfWorkContract::class;
    }
}
