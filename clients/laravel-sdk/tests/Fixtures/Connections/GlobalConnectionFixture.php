<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Fixtures\Connections;

use FlowCatalyst\Attributes\AsConnection;

#[AsConnection(code: 'orders-webhook', name: 'Orders Webhook')]
final class GlobalConnectionFixture
{
}
