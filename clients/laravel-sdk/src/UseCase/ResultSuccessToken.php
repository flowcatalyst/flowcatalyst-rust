<?php

declare(strict_types=1);

namespace FlowCatalyst\UseCase;

/**
 * Internal marker that gates `Result::success()`.
 *
 * Only UnitOfWork implementations hold an instance, so use cases cannot
 * synthesise a successful Result without going through a UnitOfWork —
 * which is what guarantees domain events get dispatched to the outbox.
 *
 * Reflection can still reach the singleton, but that requires intent —
 * same as bypassing the TypeScript SDK's `Symbol` token.
 *
 * @internal
 */
final class ResultSuccessToken
{
    private static ?self $instance = null;

    private function __construct() {}

    /**
     * @internal
     */
    public static function internal(): self
    {
        return self::$instance ??= new self();
    }
}
