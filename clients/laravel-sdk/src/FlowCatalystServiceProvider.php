<?php

declare(strict_types=1);

namespace FlowCatalyst;

use FlowCatalyst\Auth\Contracts\OidcUserHandler;
use FlowCatalyst\Auth\DefaultOidcUserHandler;
use FlowCatalyst\Client\Auth\OidcTokenManager;
use FlowCatalyst\Client\Auth\TokenProviderInterface;
use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\Console\Commands\ScanDefinitionsCommand;
use FlowCatalyst\Console\Commands\SyncDefinitionsCommand;
use FlowCatalyst\Definition\DefinitionRepository;
use FlowCatalyst\Definition\DefinitionScanner;
use FlowCatalyst\Sync\DefinitionSynchronizer;
use FlowCatalyst\Outbox\Contracts\OutboxDriver;
use FlowCatalyst\Outbox\Drivers\DatabaseDriver;
use FlowCatalyst\Outbox\Drivers\MongoDriver;
use FlowCatalyst\Outbox\OutboxManager;
use FlowCatalyst\UseCase\OutboxUnitOfWork;
use FlowCatalyst\UseCase\UnitOfWork;
use Illuminate\Support\ServiceProvider;

class FlowCatalystServiceProvider extends ServiceProvider
{
    /**
     * Register any application services.
     */
    public function register(): void
    {
        $this->mergeConfigFrom(
            __DIR__ . '/../config/flowcatalyst.php',
            'flowcatalyst'
        );

        $this->registerTokenManager();
        $this->registerClient();
        $this->registerOutbox();
        $this->registerUnitOfWork();
        $this->registerOidcUserAuth();
        $this->registerDefinitions();
    }

    /**
     * Bootstrap any application services.
     */
    public function boot(): void
    {
        $this->publishConfig();
        $this->publishMigrations();
        $this->registerMiddleware();
        $this->registerOidcRoutes();
        $this->registerCommands();
    }

    /**
     * Register the OIDC token manager.
     */
    protected function registerTokenManager(): void
    {
        $this->app->singleton(OidcTokenManager::class, function ($app) {
            $config = $app['config']['flowcatalyst'];

            return new OidcTokenManager(
                baseUrl: $config['base_url'],
                clientId: $config['client_id'] ?? '',
                clientSecret: $config['client_secret'] ?? '',
                tokenUrl: $config['token_url'],
                cache: $app['cache']->driver($config['token_cache']['driver'] ?? null),
                cacheKey: $config['token_cache']['key'] ?? 'flowcatalyst_access_token'
            );
        });

        // Bind interface to concrete implementation (allows type-hinting against interface)
        $this->app->bind(TokenProviderInterface::class, OidcTokenManager::class);
    }

    /**
     * Register the FlowCatalyst client.
     */
    protected function registerClient(): void
    {
        $this->app->singleton(FlowCatalystClient::class, function ($app) {
            $config = $app['config']['flowcatalyst'];

            return new FlowCatalystClient(
                tokenProvider: $app->make(OidcTokenManager::class),
                baseUrl: $config['base_url'],
                timeout: $config['http']['timeout'] ?? 30,
                retryAttempts: $config['http']['retry_attempts'] ?? 3,
                retryDelay: $config['http']['retry_delay'] ?? 100
            );
        });
    }

    /**
     * Register the outbox manager.
     */
    protected function registerOutbox(): void
    {
        // Register the driver based on configuration
        $this->app->singleton(OutboxDriver::class, function ($app) {
            $config = $app['config']['flowcatalyst']['outbox'];
            $driver = $config['driver'] ?? 'database';

            return match ($driver) {
                'mongodb' => new MongoDriver(
                    connection: $config['connection'],
                    collection: $config['table'] ?? 'outbox_messages'
                ),
                default => new DatabaseDriver(
                    connection: $config['connection'],
                    table: $config['table'] ?? 'outbox_messages'
                ),
            };
        });

        $this->app->singleton(OutboxManager::class, function ($app) {
            $config = $app['config']['flowcatalyst']['outbox'];

            return new OutboxManager(
                driver: $app->make(OutboxDriver::class),
                tenantId: (int) ($config['tenant_id'] ?? 0),
                defaultPartition: $config['default_partition'] ?? 'default'
            );
        });
    }

    /**
     * Register the UnitOfWork binding.
     *
     * Binds the `UnitOfWork` contract to `OutboxUnitOfWork`, wired to the
     * already-registered `OutboxManager`. Consumers type-hint `UnitOfWork`
     * in their use case constructors and Laravel injects the outbox-backed
     * implementation. Audit log emission is toggled by
     * `flowcatalyst.outbox.audit_enabled`.
     */
    protected function registerUnitOfWork(): void
    {
        $this->app->singleton(OutboxUnitOfWork::class, function ($app) {
            $config = $app['config']['flowcatalyst']['outbox'] ?? [];
            return new OutboxUnitOfWork(
                outboxManager: $app->make(OutboxManager::class),
                auditEnabled:  (bool) ($config['audit_enabled'] ?? false),
                fallbackPrincipalId: $config['fallback_principal_id'] ?? 'system',
            );
        });

        // Type-hint the contract to get the outbox-backed implementation.
        $this->app->bind(UnitOfWork::class, OutboxUnitOfWork::class);
    }

    /**
     * Publish the configuration file.
     */
    protected function publishConfig(): void
    {
        $this->publishes([
            __DIR__ . '/../config/flowcatalyst.php' => config_path('flowcatalyst.php'),
        ], 'flowcatalyst-config');
    }

    /**
     * Publish the database migrations.
     */
    protected function publishMigrations(): void
    {
        $this->publishes([
            __DIR__ . '/../database/migrations/' => database_path('migrations'),
        ], 'flowcatalyst-migrations');
    }

    /**
     * Register the webhook validation middleware.
     */
    protected function registerMiddleware(): void
    {
        $this->app['router']->aliasMiddleware(
            'flowcatalyst.webhook',
            \FlowCatalyst\Http\Middleware\ValidateWebhookSignature::class
        );
    }

    /**
     * Register the OIDC user authentication handler.
     *
     * Applications can override this by binding their own OidcUserHandler
     * implementation in their AppServiceProvider.
     */
    protected function registerOidcUserAuth(): void
    {
        // Only bind if not already bound (allows app to override)
        if (!$this->app->bound(OidcUserHandler::class)) {
            $this->app->singleton(OidcUserHandler::class, DefaultOidcUserHandler::class);
        }
    }

    /**
     * Register the definition scanner, repository, and synchronizer.
     */
    protected function registerDefinitions(): void
    {
        $this->app->singleton(DefinitionScanner::class);

        $this->app->singleton(DefinitionRepository::class, function ($app) {
            $cachePath = $app['config']['flowcatalyst']['definitions']['cache_path']
                ?? storage_path('flowcatalyst');

            return new DefinitionRepository(
                cachePath: $cachePath,
                scanner: $app->make(DefinitionScanner::class)
            );
        });

        $this->app->singleton(DefinitionSynchronizer::class, function ($app) {
            return new DefinitionSynchronizer(
                client: $app->make(FlowCatalystClient::class)
            );
        });
    }

    /**
     * Register the Artisan commands.
     */
    protected function registerCommands(): void
    {
        if ($this->app->runningInConsole()) {
            $this->commands([
                ScanDefinitionsCommand::class,
                SyncDefinitionsCommand::class,
            ]);
        }
    }

    /**
     * Register the OIDC authentication routes.
     *
     * These routes use only the minimal middleware required for session handling.
     * Auth middleware is explicitly excluded since these routes ARE the auth mechanism.
     */
    protected function registerOidcRoutes(): void
    {
        if (!config('flowcatalyst.oidc.enabled', false)) {
            return;
        }

        // Default auth middleware to exclude, plus any custom ones from config
        $excludeMiddleware = array_merge(
            ['auth', 'auth:sanctum', 'auth:api', 'auth:web'],
            config('flowcatalyst.oidc.exclude_middleware', [])
        );

        $this->app['router']->group([
            'middleware' => config('flowcatalyst.oidc.middleware', ['web']),
        ], function ($router) use ($excludeMiddleware) {
            $loginRoute = config('flowcatalyst.oidc.login_route', '/flowcatalyst/login');
            $callbackRoute = config('flowcatalyst.oidc.callback_route', '/flowcatalyst/callback');
            $logoutRoute = config('flowcatalyst.oidc.logout_route', '/flowcatalyst/logout');

            // These routes must not have auth middleware - they ARE the auth mechanism
            $router->get($loginRoute, [\FlowCatalyst\Auth\Http\Controllers\OidcAuthController::class, 'login'])
                ->name('flowcatalyst.login')
                ->withoutMiddleware($excludeMiddleware);

            $router->get($callbackRoute, [\FlowCatalyst\Auth\Http\Controllers\OidcAuthController::class, 'callback'])
                ->name('flowcatalyst.callback')
                ->withoutMiddleware($excludeMiddleware);

            $router->match(['get', 'post'], $logoutRoute, [\FlowCatalyst\Auth\Http\Controllers\OidcAuthController::class, 'logout'])
                ->name('flowcatalyst.logout')
                ->withoutMiddleware($excludeMiddleware);
        });
    }
}
