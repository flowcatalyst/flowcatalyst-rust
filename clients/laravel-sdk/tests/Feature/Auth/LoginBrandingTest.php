<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Feature\Auth;

use FlowCatalyst\FlowCatalystServiceProvider;
use Orchestra\Testbench\TestCase;

/**
 * Branded sign-in: the optional `client` identifier that names which
 * FlowCatalyst client's login theme the sign-in pages should wear.
 *
 * It is cosmetic — it selects colours and a logo, never who may sign in — so
 * the contract is deliberately forgiving: absent by default, overridable per
 * request, and never sent in portal mode, which has its own login surface.
 */
final class LoginBrandingTest extends TestCase
{
    protected function getPackageProviders($app): array
    {
        return [FlowCatalystServiceProvider::class];
    }

    protected function getEnvironmentSetUp($app): void
    {
        $app['config']->set('app.key', 'base64:'.base64_encode(str_repeat('a', 32)));
        $app['config']->set('session.driver', 'array');
        $app['config']->set('flowcatalyst.base_url', 'https://fc.test');
        $app['config']->set('flowcatalyst.oidc.enabled', true);
        $app['config']->set('flowcatalyst.oidc.client_id', 'oac_app');
    }

    /** Returns the query params of the /oauth/authorize URL the login route redirects to. */
    private function authorizeParams(string $loginUrl = '/flowcatalyst/login'): array
    {
        $response = $this->get($loginUrl);
        $response->assertRedirect();

        $location = $response->headers->get('Location');
        parse_str((string) parse_url($location, PHP_URL_QUERY), $params);
        $params['__path'] = parse_url($location, PHP_URL_PATH);

        return $params;
    }

    public function test_client_is_absent_when_not_configured(): void
    {
        $params = $this->authorizeParams();

        $this->assertArrayNotHasKey('client', $params);
        // The rest of the request is untouched.
        $this->assertSame('code', $params['response_type']);
        $this->assertSame('oac_app', $params['client_id']);
        $this->assertSame('S256', $params['code_challenge_method']);
    }

    public function test_configured_client_is_sent(): void
    {
        config(['flowcatalyst.oidc.client' => 'acme']);

        $params = $this->authorizeParams();

        $this->assertSame('acme', $params['client']);
        // `client` (tenant slug) and `client_id` (OAuth client) are distinct.
        $this->assertSame('oac_app', $params['client_id']);
    }

    public function test_per_request_client_overrides_the_configured_default(): void
    {
        config(['flowcatalyst.oidc.client' => 'acme']);

        $params = $this->authorizeParams('/flowcatalyst/login?client=other-co');

        $this->assertSame('other-co', $params['client']);
    }

    public function test_client_is_omitted_in_portal_mode(): void
    {
        config([
            'flowcatalyst.oidc.client' => 'acme',
            'flowcatalyst.oidc.portal' => true,
        ]);

        $params = $this->authorizeParams();

        $this->assertSame('/portal/authorize', $params['__path']);
        $this->assertArrayNotHasKey('client', $params);
    }

    public function test_injected_client_is_ignored_in_portal_mode(): void
    {
        config(['flowcatalyst.oidc.portal' => true]);

        $params = $this->authorizeParams('/flowcatalyst/login?client=acme');

        $this->assertArrayNotHasKey('client', $params);
    }

    public function test_client_is_url_encoded_rather_than_injected_raw(): void
    {
        config(['flowcatalyst.oidc.client' => 'a&redirect_uri=https://evil.test']);

        $response = $this->get('/flowcatalyst/login');
        $location = (string) $response->headers->get('Location');

        $this->assertStringNotContainsString('client=a&redirect_uri=https://evil.test', $location);

        parse_str((string) parse_url($location, PHP_URL_QUERY), $params);
        $this->assertStringContainsString('/flowcatalyst/callback', $params['redirect_uri']);
    }
}
