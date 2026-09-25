<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Fixtures\Connections;

use FlowCatalyst\Attributes\AsConnection;

#[AsConnection(code: 'acme-webhook', name: 'Acme Webhook', application: 'billing', client: 'acme')]
final class ScopedConnectionFixture
{
}
