<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Webhook;

use FlowCatalyst\Webhook\BearerTokenCheck;
use PHPUnit\Framework\TestCase;

/**
 * The bearer check is a LAYERED gate: unconfigured = pass-through (signature
 * alone remains the boundary); configured = the delivery must present the
 * exact token. It is never a substitute for the signature.
 */
final class BearerTokenCheckTest extends TestCase
{
    public function test_unconfigured_always_passes(): void
    {
        $this->assertNull(BearerTokenCheck::check(null, null));
        $this->assertNull(BearerTokenCheck::check('', 'Bearer whatever'));
    }

    public function test_configured_requires_exact_token(): void
    {
        $this->assertNull(BearerTokenCheck::check('fc_tok', 'Bearer fc_tok'));

        $this->assertNotNull(BearerTokenCheck::check('fc_tok', null), 'missing header');
        $this->assertNotNull(BearerTokenCheck::check('fc_tok', 'Bearer wrong'), 'wrong token');
        $this->assertNotNull(BearerTokenCheck::check('fc_tok', 'Basic fc_tok'), 'not a bearer');
        $this->assertNotNull(BearerTokenCheck::check('fc_tok', 'Bearer fc_tok2'), 'longer token');
    }
}
