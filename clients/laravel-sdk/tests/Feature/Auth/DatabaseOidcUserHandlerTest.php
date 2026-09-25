<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Feature\Auth;

use FlowCatalyst\Auth\Contracts\OidcUserHandler;
use FlowCatalyst\Auth\DatabaseOidcUserHandler;
use FlowCatalyst\Auth\DefaultOidcUserHandler;
use FlowCatalyst\Auth\DTOs\FlowCatalystUser;
use FlowCatalyst\FlowCatalystServiceProvider;
use Illuminate\Database\Schema\Blueprint;
use Illuminate\Foundation\Auth\User as Authenticatable;
use Illuminate\Support\Facades\Auth;
use Illuminate\Support\Facades\Schema;
use Orchestra\Testbench\TestCase;

/**
 * Covers the "native login bridge": with handler=database, an OIDC callback
 * upserts a local user and logs them into the stock Laravel guard, while still
 * keeping the SDK session principal and respecting app-provided overrides.
 */
final class DatabaseOidcUserHandlerTest extends TestCase
{
    protected function getPackageProviders($app): array
    {
        return [FlowCatalystServiceProvider::class];
    }

    protected function getEnvironmentSetUp($app): void
    {
        $app['config']->set('database.default', 'testing');
        $app['config']->set('database.connections.testing', [
            'driver' => 'sqlite',
            'database' => ':memory:',
            'prefix' => '',
        ]);
        $app['config']->set('session.driver', 'array');

        $app['config']->set('flowcatalyst.base_url', 'https://fc.test');
        $app['config']->set('flowcatalyst.oidc.enabled', true);
        // Native-login bridge is opt-in; this suite exercises its behaviour
        // (database handler + guest redirect), so turn it on.
        $app['config']->set('flowcatalyst.oidc.native_login', true);
        $app['config']->set('flowcatalyst.oidc.handler', 'database');
        $app['config']->set('flowcatalyst.oidc.user_model', TestUser::class);

        // Point the web guard's provider at our throwaway user model.
        $app['config']->set('auth.providers.users.model', TestUser::class);
    }

    protected function setUp(): void
    {
        parent::setUp();

        Schema::create('users', function (Blueprint $table): void {
            $table->id();
            $table->string('name')->nullable();
            $table->string('email')->unique();
            $table->string('password')->nullable();
            $table->timestamps();
        });
    }

    public function test_default_handler_is_database_when_configured(): void
    {
        $this->assertInstanceOf(
            DatabaseOidcUserHandler::class,
            $this->app->make(OidcUserHandler::class),
        );
    }

    public function test_login_upserts_user_and_logs_into_native_guard(): void
    {
        $result = $this->app->make(OidcUserHandler::class)
            ->handleAuthenticatedUser($this->fcUser('jane@example.com', 'Jane Doe'));

        $this->assertInstanceOf(TestUser::class, $result);
        $this->assertTrue(Auth::check());
        $this->assertSame('jane@example.com', Auth::user()->email);
        $this->assertDatabaseHas('users', ['email' => 'jane@example.com', 'name' => 'Jane Doe']);

        // SDK session principal preserved so the fc.* middleware keep working.
        $this->assertNotNull(session(DefaultOidcUserHandler::SESSION_KEY));
    }

    public function test_existing_user_is_reused_and_name_refreshed(): void
    {
        TestUser::create(['email' => 'sam@example.com', 'name' => 'Old Name', 'password' => bcrypt('x')]);

        $this->app->make(OidcUserHandler::class)
            ->handleAuthenticatedUser($this->fcUser('sam@example.com', 'New Name'));

        $this->assertSame(1, TestUser::where('email', 'sam@example.com')->count());
        $this->assertSame('New Name', TestUser::where('email', 'sam@example.com')->value('name'));
    }

    public function test_explicit_app_binding_wins_over_sdk_default(): void
    {
        // An app binding its own handler must override the SDK default.
        $this->app->singleton(OidcUserHandler::class, DefaultOidcUserHandler::class);

        $this->assertInstanceOf(
            DefaultOidcUserHandler::class,
            $this->app->make(OidcUserHandler::class),
        );
    }

    public function test_logout_clears_the_native_session(): void
    {
        $handler = $this->app->make(OidcUserHandler::class);
        $handler->handleAuthenticatedUser($this->fcUser('out@example.com', 'Logout User'));
        $this->assertTrue(Auth::check());

        $handler->handleLogout();
        $this->assertFalse(Auth::check());
    }

    public function test_guest_redirect_callback_targets_oidc_login(): void
    {
        // The SDK registers a guest-redirect callback at boot so stock `auth`
        // routes bounce guests into the OIDC flow. We assert the registered
        // callback directly: in a real request the framework installs its own
        // `route('login')` default when the HTTP kernel resolves (which, via
        // Application::handleRequest, happens BEFORE providers boot), so the
        // SDK's boot-time override wins. Testbench resolves the kernel AFTER
        // boot — inverting that order — so the authoritative end-to-end HTTP
        // check lives in the example app, not here.
        $property = new \ReflectionProperty(\Illuminate\Auth\Middleware\Authenticate::class, 'redirectToCallback');
        $property->setAccessible(true);
        $callback = $property->getValue();

        $this->assertNotNull($callback, 'guest redirect should be registered when OIDC is enabled');
        $this->assertSame(route('flowcatalyst.login'), $callback(request()));
    }

    private function fcUser(string $email, string $name): FlowCatalystUser
    {
        return new FlowCatalystUser(
            sub: 'usr_'.substr(md5($email), 0, 8),
            email: $email,
            name: $name,
            claims: ['sub' => 'usr', 'email' => $email, 'name' => $name, 'roles' => []],
        );
    }
}

/**
 * Throwaway Authenticatable backed by the in-memory `users` table.
 */
class TestUser extends Authenticatable
{
    protected $table = 'users';

    protected $guarded = [];
}
