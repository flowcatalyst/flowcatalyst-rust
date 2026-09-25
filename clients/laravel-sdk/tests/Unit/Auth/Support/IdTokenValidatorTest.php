<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Auth\Support;

use FlowCatalyst\Auth\Support\IdTokenValidator;
use FlowCatalyst\Auth\Support\JwksCache;
use FlowCatalyst\Tests\Support\MockIssuer;
use GuzzleHttp\Client;
use Illuminate\Cache\ArrayStore;
use Illuminate\Cache\Repository;
use PHPUnit\Framework\TestCase;

/**
 * Covers P1: OidcAuthController::parseIdToken used to trust an id_token's
 * claims after checking only `exp`/`sub`/an email-ish claim — the whole
 * principal (roles included) was built from an otherwise-unverified token.
 * IdTokenValidator is what parseIdToken now delegates to.
 */
final class IdTokenValidatorTest extends TestCase
{
    private MockIssuer $issuer;
    private const CLIENT_ID = 'test-client';

    protected function setUp(): void
    {
        $this->issuer = new MockIssuer();
    }

    protected function tearDown(): void
    {
        $this->issuer->stop();
    }

    private function validator(): IdTokenValidator
    {
        $jwks = new JwksCache(new Client(), new Repository(new ArrayStore()));
        return new IdTokenValidator($jwks, $this->issuer->baseUrl, self::CLIENT_ID);
    }

    private function baseClaims(array $overrides = []): array
    {
        $now = time();
        return array_merge([
            'sub' => 'usr_1',
            'iss' => $this->issuer->issuer,
            'aud' => self::CLIENT_ID,
            'exp' => $now + 300,
            'nbf' => $now - 5,
            'iat' => $now,
            'email' => 'jane@example.com',
            'name' => 'Jane Doe',
            'roles' => ['integral:administrator'],
            'clients' => ['clt_abc:acme'],
            'applications' => ['app_1:integral'],
        ], $overrides);
    }

    /** P1c (validator half): a correctly signed id_token verifies and its claims come back intact. */
    public function test_correctly_signed_token_is_accepted_with_its_claims(): void
    {
        $token = $this->issuer->sign($this->baseClaims());

        $claims = $this->validator()->validate($token);

        $this->assertNotNull($claims);
        $this->assertSame(['integral:administrator'], $claims['roles']);
        $this->assertSame('usr_1', $claims['sub']);
    }

    /**
     * P1a: an id_token signed by a key the issuer never registered is
     * refused, even though its header claims a `kid` the JWKS DOES contain
     * (only a signature check catches this — a kid-lookup check alone would
     * not, since the kid is real).
     */
    public function test_signature_from_unregistered_key_is_refused(): void
    {
        $token = $this->issuer->signWithRogueKey($this->baseClaims());

        $this->assertNull($this->validator()->validate($token));
    }

    /** P1b (part 1): wrong `iss` is refused even with a genuine signature. */
    public function test_wrong_issuer_is_refused(): void
    {
        $token = $this->issuer->sign($this->baseClaims(['iss' => 'https://not-the-real-issuer.test']));

        $this->assertNull($this->validator()->validate($token));
    }

    /** P1b (part 2): wrong `aud` is refused even with a genuine signature and correct `iss`. */
    public function test_wrong_audience_is_refused(): void
    {
        $token = $this->issuer->sign($this->baseClaims(['aud' => 'someone-elses-client']));

        $this->assertNull($this->validator()->validate($token));
    }

    /**
     * P1b (part 3): a token carrying NO `aud` at all is refused. Distinct
     * from the wrong-`aud` case above: a check written as "reject when aud
     * is present and does not match" would pass that test and still accept
     * this token.
     */
    public function test_token_with_no_audience_is_refused(): void
    {
        $claims = $this->baseClaims();
        unset($claims['aud']);
        $token = $this->issuer->sign($claims);

        $this->assertNull($this->validator()->validate($token));
    }

    public function test_expired_token_is_refused(): void
    {
        $token = $this->issuer->sign($this->baseClaims(['exp' => time() - 10]));

        $this->assertNull($this->validator()->validate($token));
    }

    public function test_not_yet_valid_token_beyond_leeway_is_refused(): void
    {
        // nbf 61s in the future — outside the 60s leeway.
        $token = $this->issuer->sign($this->baseClaims(['nbf' => time() + 61]));

        $this->assertNull($this->validator()->validate($token));
    }

    public function test_not_yet_valid_token_within_leeway_is_accepted(): void
    {
        // nbf 59s in the future — inside the 60s leeway.
        $token = $this->issuer->sign($this->baseClaims(['nbf' => time() + 59]));

        $this->assertNotNull($this->validator()->validate($token));
    }
}
