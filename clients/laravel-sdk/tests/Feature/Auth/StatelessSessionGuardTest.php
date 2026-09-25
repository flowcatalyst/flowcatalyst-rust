<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Feature\Auth;

use FlowCatalyst\Auth\DefaultOidcUserHandler;
use FlowCatalyst\Auth\FlowCatalystAuthenticatable;
use FlowCatalyst\FlowCatalystServiceProvider;
use Orchestra\Testbench\TestCase;

/**
 * The stateless browser-login path: the `fc-session` guard rebuilds a
 * FlowCatalystAuthenticatable from the OIDC session principal, with permissions
 * read off the stored access token's `scope` claim — no users row, no Spatie.
 */
final class StatelessSessionGuardTest extends TestCase
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
        // default permission_resolver = 'token' (stateless)
    }

    protected function defineRoutes($router): void
    {
        $router->middleware('web')->group(function ($r): void {
            $r->get('/whoami', function () {
                $user = auth('fc-session')->user();

                return [
                    'id' => $user?->getAuthIdentifier(),
                    'can_view' => (bool) $user?->hasPermissionTo('blog:posts:post:view'),
                    'can_delete' => (bool) $user?->hasPermissionTo('blog:posts:post:delete'),
                    'is_fc' => $user instanceof FlowCatalystAuthenticatable,
                ];
            });
        });
    }

    private function jwt(array $claims): string
    {
        $b64 = static fn (array $a): string => rtrim(strtr(base64_encode(json_encode($a)), '+/', '-_'), '=');
        return $b64(['alg' => 'RS256']) . '.' . $b64($claims) . '.sig';
    }

    public function test_session_principal_resolves_with_token_scope_permissions(): void
    {
        $accessToken = $this->jwt([
            'sub' => 'usr_1',
            'scope' => 'blog:posts:post:view blog:posts:post:edit',
        ]);

        $this->withSession([
            // The flat array shape handleAuthenticatedUser actually stores
            // (never the DTO itself — Laravel's session 'serialization' can
            // be 'json', under which a stored object always reads back as a
            // plain array, never a reconstructed instance).
            DefaultOidcUserHandler::SESSION_KEY => [
                'sub' => 'usr_1',
                'email' => 'jane@example.com',
                'name' => 'Jane',
                'claims' => ['clients' => [], 'roles' => ['blog:editor']],
                'access_token' => $accessToken,
                'refresh_token' => 'refresh-xyz',
                // Not expired — SessionFreshnessGuard treats a missing/past
                // exp as "needs refresh", which would otherwise mask what
                // this test is actually about (token-scope permissions).
                'access_token_claims' => ['exp' => time() + 3600, 'iat' => time()],
            ],
        ])->get('/whoami')->assertOk()->assertExactJson([
            'id' => 'usr_1',
            'can_view' => true,
            'can_delete' => false,
            'is_fc' => true,
        ]);
    }

    public function test_no_session_principal_is_unauthenticated(): void
    {
        $this->get('/whoami')->assertOk()->assertExactJson([
            'id' => null,
            'can_view' => false,
            'can_delete' => false,
            'is_fc' => false,
        ]);
    }
}
