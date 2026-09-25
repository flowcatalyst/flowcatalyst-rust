<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Feature\Auth;

use FlowCatalyst\Auth\DefaultOidcUserHandler;
use FlowCatalyst\FlowCatalystServiceProvider;
use FlowCatalyst\Tests\Support\MockIssuer;
use Orchestra\Testbench\TestCase;

/**
 * End-to-end OIDC callback coverage against a real local issuer (genuine
 * RS256 keypair) — the SDK's Guzzle client is not dependency-injected, so
 * the token exchange is real HTTP to {@see MockIssuer}, exactly the shape
 * production traffic takes.
 *
 * Covers:
 *  - P1: parseIdToken() used to trust the id_token's claims after checking
 *    only exp/sub/an email-ish claim. A correctly signed token must still
 *    log in and carry its roles (P1c) — the companion refusal cases (P1a/
 *    P1b) are pinned at the IdTokenValidator unit level, since that is
 *    exactly where those checks live.
 *  - P4: `returnTo` (what the guards send) must be read by login(), and
 *    both it and the deprecated `return_url` alias must be validated as a
 *    same-origin relative path before ever being used as a redirect target
 *    (open-redirect guard) — applied at the callback's final redirect.
 */
final class OidcCallbackSecurityTest extends TestCase
{
    private const CLIENT_ID = 'test-client';

    /**
     * OidcAuthController::RETURN_URL_SESSION_KEY is private — this SDK-internal
     * session key is the contract between login() (writer) and callback()
     * (reader), so tests that exercise only one half hardcode it here rather
     * than reaching into the controller via reflection.
     */
    private const RETURN_URL_SESSION_KEY = 'flowcatalyst_oidc_return_url';

    private MockIssuer $issuer;

    protected function setUp(): void
    {
        // MUST run before parent::setUp() — Testbench's CreatesApplication
        // calls getEnvironmentSetUp($app) (which reads $this->issuer) from
        // inside parent::setUp() itself.
        $this->issuer = new MockIssuer();
        parent::setUp();
    }

    protected function tearDown(): void
    {
        $this->issuer->stop();
        parent::tearDown();
    }

    protected function getPackageProviders($app): array
    {
        return [FlowCatalystServiceProvider::class];
    }

    protected function getEnvironmentSetUp($app): void
    {
        $app['config']->set('app.key', 'base64:'.base64_encode(str_repeat('a', 32)));
        $app['config']->set('session.driver', 'array');
        $app['config']->set('flowcatalyst.base_url', $this->issuer->baseUrl);
        $app['config']->set('flowcatalyst.oidc.enabled', true);
        $app['config']->set('flowcatalyst.oidc.client_id', self::CLIENT_ID);
    }

    private function idTokenClaims(array $overrides = []): array
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
            'nonce' => 'nonce-1',
            'roles' => ['integral:administrator'],
            'clients' => ['clt_abc:acme'],
            'applications' => ['app_1:integral'],
        ], $overrides);
    }

    private function accessTokenClaims(array $overrides = []): array
    {
        $now = time();
        return array_merge([
            'sub' => 'usr_1',
            'iss' => $this->issuer->issuer,
            'aud' => 'flowcatalyst',
            'exp' => $now + 3600,
            'iat' => $now,
            'token_use' => 'identity',
        ], $overrides);
    }

    private function queueLogin(string $code, array $idClaimOverrides = []): void
    {
        $this->issuer->queueAuthCode($code, [
            'access_token' => $this->issuer->sign($this->accessTokenClaims()),
            'id_token' => $this->issuer->sign($this->idTokenClaims($idClaimOverrides)),
            'refresh_token' => 'rt_' . $code,
            'token_type' => 'Bearer',
            'expires_in' => 3600,
        ]);
    }

    private function baseCallbackSession(array $extra = []): array
    {
        return array_merge([
            'flowcatalyst_oidc_state' => 'state-1',
            'flowcatalyst_oidc_verifier' => 'verifier-1',
            'flowcatalyst_oidc_nonce' => 'nonce-1',
        ], $extra);
    }

    /** P1c: a correctly signed id_token logs in and the principal carries its roles. */
    public function test_correctly_signed_id_token_logs_in_with_roles(): void
    {
        $this->queueLogin('code-1');

        $response = $this->withSession($this->baseCallbackSession())
            ->get('/flowcatalyst/callback?code=code-1&state=state-1');

        $response->assertRedirect('/dashboard');
        $response->assertSessionHas(
            DefaultOidcUserHandler::SESSION_KEY,
            function (array $stored): bool {
                return $stored['sub'] === 'usr_1'
                    && $stored['claims']['roles'] === ['integral:administrator'];
            },
        );
    }

    /** P4a: `returnTo` is read by login() (what RequireSession/RequireAuth send). */
    public function test_login_reads_returnTo_query_param(): void
    {
        $response = $this->get('/flowcatalyst/login?returnTo=' . urlencode('/deep/link'));

        $response->assertSessionHas(self::RETURN_URL_SESSION_KEY, '/deep/link');
    }

    /** P4a: `return_url` still works as a deprecated alias. */
    public function test_login_reads_deprecated_return_url_alias(): void
    {
        $response = $this->get('/flowcatalyst/login?return_url=' . urlencode('/deep/link'));

        $response->assertSessionHas(self::RETURN_URL_SESSION_KEY, '/deep/link');
    }

    /** P4a: a validated returnTo (stored during login) is honoured on the callback redirect. */
    public function test_callback_redirects_to_stashed_returnTo(): void
    {
        $this->queueLogin('code-2');

        $response = $this->withSession($this->baseCallbackSession([
            self::RETURN_URL_SESSION_KEY => '/deep/link',
        ]))->get('/flowcatalyst/callback?code=code-2&state=state-1');

        $response->assertRedirect('/deep/link');
    }

    /** P4b: an absolute-URL returnTo falls back to the configured post-login redirect. */
    public function test_callback_rejects_absolute_url_returnTo(): void
    {
        $this->queueLogin('code-3');

        $response = $this->withSession($this->baseCallbackSession([
            self::RETURN_URL_SESSION_KEY => 'https://evil.test/x',
        ]))->get('/flowcatalyst/callback?code=code-3&state=state-1');

        $response->assertRedirect('/dashboard');
    }

    /** P4b: a protocol-relative `//host` returnTo falls back too (not just a scheme+host URL). */
    public function test_callback_rejects_protocol_relative_returnTo(): void
    {
        $this->queueLogin('code-4');

        $response = $this->withSession($this->baseCallbackSession([
            self::RETURN_URL_SESSION_KEY => '//evil.test/x',
        ]))->get('/flowcatalyst/callback?code=code-4&state=state-1');

        $response->assertRedirect('/dashboard');
    }
}
