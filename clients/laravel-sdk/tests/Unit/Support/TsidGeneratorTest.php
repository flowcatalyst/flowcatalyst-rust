<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Support;

use FlowCatalyst\Support\TsidGenerator;
use InvalidArgumentException;
use PHPUnit\Framework\TestCase;

final class TsidGeneratorTest extends TestCase
{
    public function test_generate_is_raw_13_char_tsid(): void
    {
        $id = TsidGenerator::generate();
        $this->assertSame(13, strlen($id));
        $this->assertTrue(TsidGenerator::isValid($id));
    }

    public function test_branded_tsid_is_prefix_underscore_raw(): void
    {
        $id = TsidGenerator::generateWithPrefix('cmt');
        $this->assertMatchesRegularExpression('/^cmt_[0-9A-HJKMNP-TV-Z]{13}$/i', $id);

        // The body after the prefix is a valid raw TSID.
        $this->assertTrue(TsidGenerator::isValid(substr($id, 4)));
    }

    public function test_empty_prefix_is_rejected(): void
    {
        $this->expectException(InvalidArgumentException::class);
        TsidGenerator::generateWithPrefix('');
    }

    public function test_underscore_in_prefix_is_rejected(): void
    {
        $this->expectException(InvalidArgumentException::class);
        TsidGenerator::generateWithPrefix('cm_t');
    }
}
