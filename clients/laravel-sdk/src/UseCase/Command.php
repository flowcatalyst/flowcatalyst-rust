<?php

declare(strict_types=1);

namespace FlowCatalyst\UseCase;

/**
 * Marker interface for use case input DTOs. Concrete commands implement this
 * and add their own fields (typically as readonly properties).
 *
 * @example
 * ```php
 * final class ShipOrderCommand implements Command
 * {
 *     public function __construct(
 *         public readonly string $orderId,
 *         public readonly string $trackingNumber,
 *     ) {}
 * }
 * ```
 */
interface Command
{
}
