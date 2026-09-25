<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Sync;

use FlowCatalyst\Sync\DefinitionSynchronizer;
use PHPUnit\Framework\TestCase;
use ReflectionClass;

/**
 * The event-type code is `application:subdomain:aggregate:event`. Scanned
 * `#[AsEventType]` rows carry subdomain/aggregate/event but NOT the application
 * (that's the set's app code), so the synchronizer must supply it — otherwise
 * the code is empty and the API rejects it ("must follow format …").
 */
final class EventTypeCodeTest extends TestCase
{
    /** @param array<string,mixed> $row */
    private function code(array $row, string $appCode): string
    {
        $rc = new ReflectionClass(DefinitionSynchronizer::class);
        $m = $rc->getMethod('eventTypeCode');
        $m->setAccessible(true);
        return $m->invoke($rc->newInstanceWithoutConstructor(), $row, $appCode);
    }

    public function test_scanned_row_uses_set_application(): void
    {
        $this->assertSame(
            'example:comments:comment:created',
            $this->code(['subdomain' => 'comments', 'aggregate' => 'comment', 'event' => 'created'], 'example'),
        );
    }

    public function test_row_application_overrides_set(): void
    {
        $this->assertSame(
            'orders:o:order:created',
            $this->code(['application' => 'orders', 'subdomain' => 'o', 'aggregate' => 'order', 'event' => 'created'], 'example'),
        );
    }

    public function test_explicit_code_wins(): void
    {
        $this->assertSame('x:y:z:w', $this->code(['code' => 'x:y:z:w'], 'example'));
    }
}
