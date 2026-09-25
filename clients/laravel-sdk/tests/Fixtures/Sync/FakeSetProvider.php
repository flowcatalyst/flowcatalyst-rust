<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Fixtures\Sync;

use FlowCatalyst\Sync\ConnectionDefinition;
use FlowCatalyst\Sync\ProvidesSyncDefinitionSets;
use FlowCatalyst\Sync\SyncDefinitionSet;

/**
 * Test-only provider for `flowcatalyst.definitions.set_providers` — yields a
 * single global set with one connection, so a test can assert the command
 * resolved this class from the container and synced what it produced.
 */
final class FakeSetProvider implements ProvidesSyncDefinitionSets
{
    public function syncDefinitionSets(): iterable
    {
        yield SyncDefinitionSet::forApplication('orders')
            ->withConnections([ConnectionDefinition::make('conn-1', 'Conn 1')]);
    }
}
