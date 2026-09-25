<?php

declare(strict_types=1);

namespace FlowCatalyst\UseCase;

/**
 * Thrown by {@link Runner} inside the unit of work's transaction to surface a
 * failed {@link Result} past `DB::transaction(...)` so the transaction rolls
 * back. Caught and unwrapped by the Runner — never escapes the SDK. Mirrors
 * the TypeScript SDK's `OutboxRunRollback`.
 *
 * @internal
 */
final class RollbackSignal extends \RuntimeException
{
    public function __construct(public readonly Result $result)
    {
        parent::__construct('use-case envelope: rolling back transaction on failed Result');
    }
}
