<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Feature\Auth;

use FlowCatalyst\Auth\DefaultOidcUserHandler;
use FlowCatalyst\FlowCatalystServiceProvider;
use FlowCatalyst\Tests\Support\MockIssuer;
use Orchestra\Testbench\TestCase;

/**
 * Covers P3: TokenRefresher used to rebuild the principal from the ACCESS
 * token, which for a normal client carries no authority at all (roles/
 * clients/applications live only in the id_token — see
 * TokenIssuer.identityAccessToken). An hour into a session, every refresh
 * silently wiped the principal's roles.
 *
 * Also covers P4's application to the refresh route's redirect (:258-260 in
 * the original code) — the same open-redirect guard as the callback route.
 */
final class OidcRefreshAuthorityTest extends TestCase
{
    private const CLIENT_ID = 'test-client';

    private MockIssuer $issuer;

    protected function setUp(): void
    {
        // MUST run before parent::setUp() — see OidcCallbackSecurityTest.
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

    /**
     * The session-stored principal shape DefaultOidcUserHandler actually
     * writes/reads (a flat array, never the DTO — see its own docblock).
     *
     * `claims` always carries `sub` (as a real id_token-derived claims array
     * would) — omitting it would make FlowCatalystUser::fromClaims() throw
     * on the "no id_token, keep previous claims" branch, which would make
     * the refresh silently fail and leave the ALREADY-SEEDED session value
     * untouched — a test that "passes" without ever exercising the merge
     * logic it claims to cover.
     */
    private function storedPrincipal(array $claims, string $refreshToken): array
    {
        return [
            'sub' => 'usr_1',
            'email' => 'jane@example.com',
            'name' => 'Jane Doe',
            'claims' => array_merge(['sub' => 'usr_1', 'email' => 'jane@example.com', 'name' => 'Jane Doe'], $claims),
            'access_token' => 'old-access-token-not-a-real-jwt',
            'refresh_token' => $refreshToken,
            'access_token_claims' => ['exp' => time() - 10, 'iat' => time() - 3600],
        ];
    }

    /** An access token freshly minted by the refresh grant: identity-only, no authority. */
    private function freshAccessToken(): string
    {
        $now = time();
        return $this->issuer->sign([
            'sub' => 'usr_1',
            'iss' => $this->issuer->issuer,
            'aud' => 'flowcatalyst',
            'exp' => $now + 3600,
            'iat' => $now,
            'token_use' => 'identity',
        ]);
    }

    private function idTokenWithRoles(array $roles): string
    {
        $now = time();
        return $this->issuer->sign([
            'sub' => 'usr_1',
            'iss' => $this->issuer->issuer,
            'aud' => self::CLIENT_ID,
            'exp' => $now + 300,
            'nbf' => $now - 5,
            'iat' => $now,
            'email' => 'jane@example.com',
            'name' => 'Jane Doe',
            'roles' => $roles,
            'clients' => ['clt_abc:acme'],
            'applications' => ['app_1:integral'],
        ]);
    }

    /** P3a: after a refresh whose id_token carries roles, the principal still has them. */
    public function test_refresh_with_id_token_rebuilds_roles_from_it(): void
    {
        $this->issuer->queueRefreshToken('rt_1', [
            'access_token' => $this->freshAccessToken(),
            'id_token' => $this->idTokenWithRoles(['integral:administrator', 'integral:viewer']),
            'refresh_token' => 'rt_1_rotated',
            'token_type' => 'Bearer',
            'expires_in' => 3600,
        ]);

        $response = $this->withSession([
            DefaultOidcUserHandler::SESSION_KEY => $this->storedPrincipal(
                ['roles' => ['integral:viewer']], // the PREVIOUS roles, distinct from the fresh id_token's
                'rt_1',
            ),
        ])->get('/flowcatalyst/refresh');

        $response->assertRedirect();
        $response->assertSessionHas(
            DefaultOidcUserHandler::SESSION_KEY,
            fn (array $stored): bool => $stored['access_token'] !== 'old-access-token-not-a-real-jwt'
                && $stored['claims']['roles'] === ['integral:administrator', 'integral:viewer'],
        );
    }

    /** P3b: after a refresh with NO id_token, the previous roles survive (never downgraded to empties). */
    public function test_refresh_without_id_token_keeps_previous_roles(): void
    {
        $this->issuer->queueRefreshToken('rt_2', [
            'access_token' => $this->freshAccessToken(),
            // No id_token at all — mirrors a login whose original scope did
            // not include `openid`.
            'refresh_token' => 'rt_2_rotated',
            'token_type' => 'Bearer',
            'expires_in' => 3600,
        ]);

        $response = $this->withSession([
            DefaultOidcUserHandler::SESSION_KEY => $this->storedPrincipal(
                ['roles' => ['integral:administrator'], 'clients' => ['clt_abc:acme']],
                'rt_2',
            ),
        ])->get('/flowcatalyst/refresh');

        $response->assertRedirect();
        $response->assertSessionHas(
            DefaultOidcUserHandler::SESSION_KEY,
            // The access_token check proves the refresh actually ran (and
            // wasn't, say, silently thrown away leaving the seeded session
            // value untouched, which would make the roles assertion below
            // pass vacuously).
            fn (array $stored): bool => $stored['access_token'] !== 'old-access-token-not-a-real-jwt'
                && $stored['claims']['roles'] === ['integral:administrator']
                && $stored['claims']['clients'] === ['clt_abc:acme'],
        );
    }

    /** P4 (refresh route): a validated returnTo is honoured. */
    public function test_refresh_redirects_to_returnTo(): void
    {
        $this->issuer->queueRefreshToken('rt_3', [
            'access_token' => $this->freshAccessToken(),
            'refresh_token' => 'rt_3_rotated',
            'token_type' => 'Bearer',
            'expires_in' => 3600,
        ]);

        $response = $this->withSession([
            DefaultOidcUserHandler::SESSION_KEY => $this->storedPrincipal(['roles' => []], 'rt_3'),
        ])->get('/flowcatalyst/refresh?returnTo=' . urlencode('/deep/link'));

        $response->assertRedirect('/deep/link');
    }

    /** P4 (refresh route): an absolute-URL returnTo is rejected, never redirected to. */
    public function test_refresh_rejects_absolute_url_returnTo(): void
    {
        $this->issuer->queueRefreshToken('rt_4', [
            'access_token' => $this->freshAccessToken(),
            'refresh_token' => 'rt_4_rotated',
            'token_type' => 'Bearer',
            'expires_in' => 3600,
        ]);

        $response = $this->withSession([
            DefaultOidcUserHandler::SESSION_KEY => $this->storedPrincipal(['roles' => []], 'rt_4'),
        ])->get('/flowcatalyst/refresh?returnTo=' . urlencode('https://evil.test/x'));

        $location = (string) $response->headers->get('Location');
        $this->assertStringNotContainsString('evil.test', $location);
        // The refresh route's success and error paths can both resolve their
        // fallback redirect to "/" in this test config, so a bare Location
        // check can't tell "validation correctly fell back" apart from "the
        // refresh silently failed" (both would innocently avoid evil.test).
        // `status` is flashed ONLY on the success path (refresh()'s
        // catch block flashes `error`, never `status`), so this pins that
        // the request actually took the success branch.
        $response->assertSessionHas('status', 'Session refreshed.');
    }
}
