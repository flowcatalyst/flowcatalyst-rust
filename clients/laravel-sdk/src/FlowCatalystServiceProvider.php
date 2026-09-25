<?php

declare(strict_types=1);

namespace FlowCatalyst;

use FlowCatalyst\Auth\Contracts\OidcUserHandler;
use FlowCatalyst\Auth\DatabaseOidcUserHandler;
use FlowCatalyst\Auth\DefaultOidcUserHandler;
use FlowCatalyst\Auth\Http\Middleware\AuthenticateFc;
use FlowCatalyst\Auth\Http\Middleware\RequireAuth;
use FlowCatalyst\Auth\Http\Middleware\RequireBearer;
use FlowCatalyst\Auth\Http\Middleware\RequireSession;
use FlowCatalyst\Auth\Rbac\RbacCatalogue;
use FlowCatalyst\Auth\Support\AccessTokenValidator;
use FlowCatalyst\Auth\Support\JwksCache;
use FlowCatalyst\Client\Auth\OidcTokenManager;
use FlowCatalyst\Client\Auth\TokenProviderInterface;
use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\Console\Commands\ScanDefinitionsCommand;
use FlowCatalyst\Console\Commands\SessionTableCommand;
use FlowCatalyst\Console\Commands\SyncDefinitionsCommand;
use FlowCatalyst\Console\Commands\VerifySignatureCommand;
use FlowCatalyst\Definition\DefinitionRepository;
use FlowCatalyst\Definition\DefinitionScanner;
use FlowCatalyst\Sync\DefinitionSynchronizer;
use FlowCatalyst\Outbox\Contracts\OutboxDriver;
use FlowCatalyst\Outbox\Drivers\DatabaseDriver;
use FlowCatalyst\Outbox\Drivers\MongoDriver;
use FlowCatalyst\Outbox\OutboxManager;
use FlowCatalyst\ScheduledJobs\CacheLockProvider;
use FlowCatalyst\ScheduledJobs\ScheduledJobRunner;
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
        $this->registerScheduledJobRunner();
        $this->registerOutbox();
        $this->registerUnitOfWork();
        $this->registerOidcUserAuth();
        $this->registerBearerValidator();
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
        $this->registerTokenGuard();
        $this->registerOidcRoutes();
        $this->registerScheduledJobRoute();
        $this->registerGuestRedirect();
        $this->registerCommands();
    }

    /**
     * Register the stateless `fc-token` auth guard and route Gate checks for
     * FlowCatalyst token identities through their RBAC permissions.
     *
     * Usage:
     *   - FlowCatalyst-only:        ->middleware('auth:fc-token')
     *   - FlowCatalyst OR Passport: ->middleware('auth:fc-token,api')
     *   - in front of Passport's
     *     client-credentials chain:  ->middleware('fc.or-passport')
     */
    protected function registerTokenGuard(): void
    {
        // Expose the guards by name without the app having to edit config/auth.php.
        config(['auth.guards.fc-token' => ['driver' => 'fc-token']]);
        config(['auth.guards.fc-session' => ['driver' => 'fc-session']]);

        // Bearer/API identities (user + client_credentials tokens).
        \Illuminate\Support\Facades\Auth::viaRequest('fc-token', function ($request) {
            return $this->app->make(\FlowCatalyst\Auth\FlowCatalystTokenGuard::class)->resolve($request);
        });

        // Browser/session identities (OIDC login) — stateless: no users row,
        // no Spatie. Use `auth:fc-session` or set it as the default guard.
        \Illuminate\Support\Facades\Auth::viaRequest('fc-session', function ($request) {
            return $this->app->make(\FlowCatalyst\Auth\FlowCatalystSessionGuard::class)->resolve($request);
        });

        // $user->can(...) / @can / policies → checkPermissionTo (FlowCatalyst
        // wildcard RBAC). Scoped to our identity so it never interferes with
        // Eloquent/Spatie users resolved by other guards.
        \Illuminate\Support\Facades\Gate::before(function ($user, $ability) {
            if ($user instanceof \FlowCatalyst\Auth\FlowCatalystAuthenticatable) {
                return $user->checkPermissionTo($ability) ? true : null;
            }
            return null;
        });
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

        // Bind the interface to the config-built singleton explicitly. Binding
        // to the class *string* (bind(..., OidcTokenManager::class)) relies on
        // the container cascading to the singleton above; if that cascade isn't
        // in effect it autowires OidcTokenManager instead, which can't supply
        // the scalar credentials and yields a token manager with empty creds.
        // Resolving through make() guarantees the config-driven instance.
        $this->app->singleton(TokenProviderInterface::class, fn ($app) => $app->make(OidcTokenManager::class));
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
     * Register a pre-configured ScheduledJobRunner. Zero-config overlap
     * protection: the lock provider is the app's default cache store (atomic
     * cache locks — safe across replicas that share the backend), and lock
     * enforcement follows each job definition's `concurrent` attribute as
     * echoed in the firing envelope. singletonIf so an app that constructs
     * its own runner (custom lock backend, TTLs, error hook) wins.
     */
    protected function registerScheduledJobRunner(): void
    {
        $this->app->singletonIf(ScheduledJobRunner::class, function ($app) {
            $runner = new ScheduledJobRunner(
                $app->make(FlowCatalystClient::class)->scheduledJobs(),
                ['lockProvider' => new CacheLockProvider($app->make('cache.store'))],
            );

            // Zero-HTTP handler wiring: scanned #[AsScheduledJob] classes with
            // a handle() method (HandlesScheduledJob) are registered
            // automatically, keyed by their declared code — the same code the
            // firing envelope carries. Apps that call $runner->handler()
            // themselves keep full control (manual registrations win; disable
            // wholesale via flowcatalyst.scheduled_jobs.auto_handlers). A scan
            // failure must not break firings for manually-registered codes.
            $auto = $app['config']['flowcatalyst']['scheduled_jobs']['auto_handlers'] ?? true;
            if ($auto) {
                try {
                    ScheduledJobRunner::registerScannedHandlers(
                        $runner,
                        $app->make(DefinitionRepository::class)->scheduledJobs(),
                        static fn (string $class): object => $app->make($class),
                    );
                } catch (\Throwable $e) {
                    report($e);
                }
            }
            return $runner;
        });
    }

    /**
     * The absolute URL the platform POSTs scheduled-job firings to. Explicit
     * config wins; otherwise APP_URL + the runner's conventional path. Null
     * only when neither is configured (the sync then omits targetUrl, as
     * before).
     */
    protected function scheduledJobProcessUrl($app): ?string
    {
        $explicit = $app['config']['flowcatalyst']['scheduled_jobs']['process_url'] ?? null;
        if (is_string($explicit) && $explicit !== '') {
            return $explicit;
        }
        $base = $app['config']['app.url'] ?? null;
        if (!is_string($base) || $base === '') {
            return null;
        }
        return rtrim($base, '/') . ScheduledJobRunner::DEFAULT_PROCESS_PATH;
    }

    /**
     * The base URL a subscription's path-style target is resolved against.
     * Explicit config wins; otherwise APP_URL. Null when neither is set — the
     * sync then rejects any subscription whose target is a path.
     */
    protected function subscriptionTargetBaseUrl($app): ?string
    {
        $explicit = $app['config']['flowcatalyst']['subscriptions']['target_base_url'] ?? null;
        if (is_string($explicit) && $explicit !== '') {
            return $explicit;
        }
        $base = $app['config']['app.url'] ?? null;
        return is_string($base) && $base !== '' ? $base : null;
    }

    /**
     * Mount the conventional scheduled-job process endpoint, wired to the
     * container's ScheduledJobRunner — apps only register handlers. Disable
     * via `flowcatalyst.scheduled_jobs.register_route` to mount your own
     * route (e.g. under extra middleware), in which case set `process_url`
     * (or per-job targetUrl) to match.
     *
     * Guarded by the fc-signature validator, FAIL-CLOSED: the platform signs
     * every firing with the application service account's signing secret, and
     * an app without `flowcatalyst.signing_secret` (FLOWCATALYST_SIGNING_SECRET)
     * rejects all deliveries with a descriptive 401 that lands in the
     * platform's delivery_error — the missing pairing is diagnosable, never
     * silent. The signature is computed over the RAW request body, so this
     * route must stay out of any body-mutating middleware groups.
     */
    protected function registerScheduledJobRoute(): void
    {
        $enabled = $this->app['config']['flowcatalyst']['scheduled_jobs']['register_route'] ?? true;
        if (!$enabled) {
            return;
        }
        // Controller class, not a closure: `php artisan route:cache` cannot
        // serialize closure routes (a provider-registered closure fatals the
        // cache with infinite recursion on its captured scope).
        \Illuminate\Support\Facades\Route::post(
            ScheduledJobRunner::DEFAULT_PROCESS_PATH,
            \FlowCatalyst\Http\Controllers\ScheduledJobProcessController::class,
        )->middleware(\FlowCatalyst\Http\Middleware\ValidateWebhookSignature::class)
            ->name('flowcatalyst.scheduled-jobs.process');
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
                    table: $config['table'] ?? 'outbox_messages',
                    strictTransactions: (bool) ($config['strict_transactions'] ?? false),
                ),
            };
        });

        $this->app->singleton(OutboxManager::class, function ($app) {
            $config = $app['config']['flowcatalyst']['outbox'];

            return new OutboxManager(
                driver: $app->make(OutboxDriver::class),
                // The FlowCatalyst client that owns these outbox rows. Defaults
                // to the configured service-account client id; override with
                // FLOWCATALYST_OUTBOX_CLIENT_ID.
                clientId: (string) ($config['client_id']
                    ?? $app['config']['flowcatalyst']['client_id']
                    ?? ''),
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
                // Same connection the outbox DatabaseDriver uses, so the owned
                // transaction (Runner -> transaction()) covers the event insert.
                connection: $config['connection'] ?? null,
                // Application + client linkage for emitted events / audit logs.
                applicationCode: $app['config']['flowcatalyst']['application_code'] ?? null,
                clientCode: $config['client_code'] ?? null,
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
     * Publish the OPT-IN database migrations — currently just the outbox table
     * (apps using the database outbox driver). Publish-only; the SDK never
     * auto-runs it.
     *
     * NB: the `sessions.user_id` widening is NOT a publishable migration — it's
     * the `flowcatalyst:session-table` command (idempotent, with --rollback).
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
        $router = $this->app['router'];
        $router->aliasMiddleware(
            'flowcatalyst.webhook',
            \FlowCatalyst\Http\Middleware\ValidateWebhookSignature::class,
        );
        // Same validator under the name the scheduled-job handover doc
        // standardises on; guards any route receiving platform-signed POSTs.
        $router->aliasMiddleware(
            'fc-signature',
            \FlowCatalyst\Http\Middleware\ValidateWebhookSignature::class,
        );
        // Auth middleware aliases — match the TS plugin's guard names.
        $router->aliasMiddleware('fc.auth', AuthenticateFc::class);
        $router->aliasMiddleware('fc.session', RequireSession::class);
        $router->aliasMiddleware('fc.bearer', RequireBearer::class);
        $router->aliasMiddleware('fc.any', RequireAuth::class);
        $router->aliasMiddleware(
            'fc.or-passport',
            \FlowCatalyst\Auth\Http\Middleware\AuthenticateServiceTokenOrFallback::class,
        );
    }

    /**
     * Register the Bearer-token validator + (optional) RBAC catalogue.
     *
     * Apps register their own RBAC catalogue by binding {@see RbacCatalogue}
     * in their `AppServiceProvider`:
     *
     *   $this->app->instance(RbacCatalogue::class, RbacBuilder::make()
     *       ->role('billing-admin')->grants(['invoice:*'])
     *       ->build());
     */
    protected function registerBearerValidator(): void
    {
        $this->app->singleton(JwksCache::class, function ($app) {
            $config = $app['config']['flowcatalyst'];
            $ttl = (int) ($config['oidc']['jwks_ttl_seconds'] ?? 300);
            return new JwksCache(
                http: new \GuzzleHttp\Client(['timeout' => 10]),
                cache: $app['cache']->driver($config['oidc']['jwks_cache_driver'] ?? null),
                ttlSeconds: $ttl,
            );
        });

        $this->app->singleton(AccessTokenValidator::class, function ($app) {
            $config = $app['config']['flowcatalyst'];
            return new AccessTokenValidator(
                jwks: $app->make(JwksCache::class),
                baseUrl: $config['base_url'],
                expectedAudience: $config['oidc']['expected_audience'] ?? null,
            );
        });

        // Verifies OIDC id_tokens (login + refresh) — separate from
        // AccessTokenValidator because `aud` is mandatory here (the SDK's own
        // OIDC client_id), never the opt-in `expected_audience`.
        $this->app->singleton(\FlowCatalyst\Auth\Support\IdTokenValidator::class, function ($app) {
            $config = $app['config']['flowcatalyst'];
            return new \FlowCatalyst\Auth\Support\IdTokenValidator(
                jwks: $app->make(JwksCache::class),
                baseUrl: $config['base_url'],
                clientId: (string) ($config['oidc']['client_id'] ?? ''),
            );
        });

        $this->app->singleton(\FlowCatalyst\Auth\Support\TokenRefresher::class, function ($app) {
            return new \FlowCatalyst\Auth\Support\TokenRefresher(
                userHandler: $app->make(\FlowCatalyst\Auth\Contracts\OidcUserHandler::class),
                idTokenValidator: $app->make(\FlowCatalyst\Auth\Support\IdTokenValidator::class),
            );
        });

        $this->app->singleton(\FlowCatalyst\Auth\Support\RevocationChecker::class, function ($app) {
            $config = $app['config']['flowcatalyst'];
            return new \FlowCatalyst\Auth\Support\RevocationChecker(
                http: new \GuzzleHttp\Client(['timeout' => 10]),
                cache: $app['cache']->driver($config['oidc']['revocation_cache_driver'] ?? null),
                baseUrl: $config['base_url'],
                ttlSeconds: (int) ($config['oidc']['revocation_cache_ttl_seconds'] ?? 60),
            );
        });

        $this->app->singleton(\FlowCatalyst\Auth\Support\SessionFreshnessGuard::class, function ($app) {
            return new \FlowCatalyst\Auth\Support\SessionFreshnessGuard(
                refresher: $app->make(\FlowCatalyst\Auth\Support\TokenRefresher::class),
                revocationChecker: $app->make(\FlowCatalyst\Auth\Support\RevocationChecker::class),
            );
        });

        $this->app->singleton(AuthenticateFc::class, function ($app) {
            return new AuthenticateFc(
                validator: $app->make(AccessTokenValidator::class),
                rbac: $app->bound(RbacCatalogue::class) ? $app->make(RbacCatalogue::class) : null,
                freshness: $app->make(\FlowCatalyst\Auth\Support\SessionFreshnessGuard::class),
            );
        });

        $this->app->singleton(\FlowCatalyst\Auth\Support\ApiMePermissionResolver::class, function ($app) {
            $config = $app['config']['flowcatalyst'];
            return new \FlowCatalyst\Auth\Support\ApiMePermissionResolver(
                http: new \GuzzleHttp\Client(['timeout' => 10]),
                cache: $app['cache']->driver($config['oidc']['me_cache_driver'] ?? null),
                baseUrl: $config['base_url'],
                ttlSeconds: (int) ($config['oidc']['me_cache_ttl_seconds'] ?? 60),
            );
        });

        // Default PermissionResolver — apps can bind their own
        // (FlowCatalyst\Auth\Contracts\PermissionResolver) to override. With a
        // local RbacCatalogue bound we use it (offline); otherwise the
        // server-backed /api/me resolver.
        $this->app->bindIf(\FlowCatalyst\Auth\Contracts\PermissionResolver::class, function ($app) {
            // A locally-bound RBAC catalogue (offline) always wins.
            if ($app->bound(RbacCatalogue::class)) {
                return new \FlowCatalyst\Auth\Rbac\CataloguePermissionResolver($app->make(RbacCatalogue::class));
            }

            // Default: read permissions straight off the token's `scope` claim
            // (stateless, no HTTP/DB). 'api_me' opts into server-side /api/me
            // resolution (one cached HTTP call, fresher on permission changes).
            return match (config('flowcatalyst.oidc.permission_resolver', 'token')) {
                'api_me' => $app->make(\FlowCatalyst\Auth\Support\ApiMePermissionResolver::class),
                default => $app->make(\FlowCatalyst\Auth\Support\TokenScopePermissionResolver::class),
            };
        });

        $this->app->singleton(\FlowCatalyst\Auth\FlowCatalystTokenGuard::class, function ($app) {
            return new \FlowCatalyst\Auth\FlowCatalystTokenGuard(
                validator: $app->make(AccessTokenValidator::class),
                permissions: $app->make(\FlowCatalyst\Auth\Contracts\PermissionResolver::class),
            );
        });

        // Session counterpart of the token guard — rebuilds the stateless
        // FlowCatalystAuthenticatable from the OIDC session principal.
        $this->app->singleton(\FlowCatalyst\Auth\FlowCatalystSessionGuard::class, function ($app) {
            return new \FlowCatalyst\Auth\FlowCatalystSessionGuard(
                permissions: $app->make(\FlowCatalyst\Auth\Contracts\PermissionResolver::class),
                freshness: $app->make(\FlowCatalyst\Auth\Support\SessionFreshnessGuard::class),
            );
        });
    }

    /**
     * Register the OIDC user authentication handler.
     *
     * The default is selected by `flowcatalyst.oidc.handler`:
     *   - 'database' (default) — {@see DatabaseOidcUserHandler}: upserts a local
     *     user and logs them into the native Laravel guard so stock `auth`
     *     middleware works out of the box;
     *   - 'session' — {@see DefaultOidcUserHandler}: session principal only.
     *
     * Bound with `if (!bound)` so an app that binds its own OidcUserHandler in
     * its service provider (e.g. for tenant checks) always wins.
     */
    protected function registerOidcUserAuth(): void
    {
        // Only bind if not already bound (allows app to override). Resolve the
        // handler class LAZILY (at make() time) so it reads the final config,
        // not whatever was set at registration time.
        if (!$this->app->bound(OidcUserHandler::class)) {
            $this->app->singleton(OidcUserHandler::class, function ($app) {
                // Default follows the native-login bridge: 'database' (upsert +
                // native login) when the bridge is on, else 'session'
                // (principal-only). An explicit `oidc.handler` pins it.
                $default = config('flowcatalyst.oidc.native_login', false) ? 'database' : 'session';
                $mode = config('flowcatalyst.oidc.handler') ?: $default;

                return $app->make($mode === 'session'
                    ? DefaultOidcUserHandler::class
                    : DatabaseOidcUserHandler::class);
            });
        }
    }

    /**
     * Make the stock `auth` middleware redirect guests into the FlowCatalyst
     * OIDC flow, so a fresh app needs no `login` route or custom redirect.
     *
     * Safe by construction — it cannot disturb an app that has its own auth:
     *  - opt out entirely via `oidc.auto_guest_redirect = false`;
     *  - never overrides a redirect callback another provider already set;
     *  - defers to the app's own `login` route when one exists (won't hijack a
     *    local login page — evaluated lazily, after routes are loaded);
     *  - an app whose custom Authenticate middleware overrides redirectTo()
     *    bypasses this callback completely.
     */
    protected function registerGuestRedirect(): void
    {
        if (!config('flowcatalyst.oidc.enabled', false)) {
            return;
        }
        // Bridge-only: never touch a host app's guest redirect unless it opted
        // into the native-login bridge. Existing apps with their own auth keep
        // their own redirect behaviour.
        if (!config('flowcatalyst.oidc.native_login', false)) {
            return;
        }
        if (!config('flowcatalyst.oidc.auto_guest_redirect', true)) {
            return;
        }

        // NB: on Laravel 11+ the framework installs its own default guest
        // redirect (`fn () => route('login')`) when the HTTP kernel resolves —
        // which happens BEFORE service providers boot (Application::handleRequest
        // resolves the kernel, then Kernel::handle() boots providers). So this
        // boot-time call runs AFTER the framework default and overrides it. We
        // deliberately do NOT guard on "is a callback already set" — that would
        // always see the framework default and bail, leaving a `route('login')`
        // that 500s on an app with no `login` route.
        //
        // App-safety instead lives in the callback: if the app has its own
        // `login` route we return that (identical to the framework default, so
        // a local login page is never hijacked); only a login-less app is sent
        // to FlowCatalyst. Apps wanting different behaviour set their own
        // redirect in bootstrap/app.php and disable `oidc.auto_guest_redirect`.
        \Illuminate\Auth\Middleware\Authenticate::redirectUsing(function ($request) {
            if (\Illuminate\Support\Facades\Route::has('login')) {
                return route('login');
            }
            if (\Illuminate\Support\Facades\Route::has('flowcatalyst.login')) {
                return route('flowcatalyst.login');
            }
            return null;
        });
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
                client: $app->make(FlowCatalystClient::class),
                defaultScheduledJobTargetUrl: $this->scheduledJobProcessUrl($app),
                subscriptionTargetBaseUrl: $this->subscriptionTargetBaseUrl($app),
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
                SessionTableCommand::class,
                VerifySignatureCommand::class,
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

            // Refresh the session principal from the stored refresh token (picks
            // up new permissions without a full re-login).
            $refreshRoute = config('flowcatalyst.oidc.refresh_route', '/flowcatalyst/refresh');
            $router->match(['get', 'post'], $refreshRoute, [\FlowCatalyst\Auth\Http\Controllers\OidcAuthController::class, 'refresh'])
                ->name('flowcatalyst.refresh')
                ->withoutMiddleware($excludeMiddleware);
        });
    }
}
