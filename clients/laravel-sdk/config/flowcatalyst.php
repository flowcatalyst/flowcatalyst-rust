<?php

return [
    /*
    |--------------------------------------------------------------------------
    | FlowCatalyst API Base URL
    |--------------------------------------------------------------------------
    |
    | The base URL for the FlowCatalyst platform API. This is typically your
    | FlowCatalyst instance URL.
    |
    */
    'base_url' => env('FLOWCATALYST_BASE_URL', 'https://api.flowcatalyst.io'),

    /*
    |--------------------------------------------------------------------------
    | Service Account Credentials
    |--------------------------------------------------------------------------
    |
    | These credentials are used to authenticate with the FlowCatalyst API
    | using the OAuth2 client credentials grant flow.
    |
    | For single-application deployments:
    |   Use a service account tied to your application.
    |
    | For multi-application deployments:
    |   Use a service account with access to multiple applications. Sync
    |   definitions programmatically using the DefinitionSynchronizer:
    |
    |   $synchronizer->syncAll([
    |       SyncDefinitionSet::forApplication('app-one')->withEventTypes([...]),
    |       SyncDefinitionSet::forApplication('app-two')->withEventTypes([...]),
    |   ]);
    |
    */
    'client_id' => env('FLOWCATALYST_CLIENT_ID'),
    'client_secret' => env('FLOWCATALYST_CLIENT_SECRET'),

    /*
    |--------------------------------------------------------------------------
    | Token URL
    |--------------------------------------------------------------------------
    |
    | The OAuth2 token endpoint. Defaults to {base_url}/oauth/token if not set.
    |
    */
    'token_url' => env('FLOWCATALYST_TOKEN_URL'),

    /*
    |--------------------------------------------------------------------------
    | Webhook Signing Secret
    |--------------------------------------------------------------------------
    |
    | The secret used to validate incoming webhook signatures from FlowCatalyst.
    | This should match the signing secret from your service account.
    |
    */
    'signing_secret' => env('FLOWCATALYST_SIGNING_SECRET'),

    /*
    |--------------------------------------------------------------------------
    | Webhook Bearer Token (optional second gate)
    |--------------------------------------------------------------------------
    |
    | Platform deliveries also carry `Authorization: Bearer <token>` — your
    | service account's webhook auth token, shown next to the signing secret.
    | When set, the fc-signature middleware requires BOTH the signature and
    | this bearer to match (layered, never instead of the signature). Leave
    | unset to validate the signature only.
    |
    */
    'webhook_auth_token' => env('FLOWCATALYST_WEBHOOK_AUTH_TOKEN'),

    /*
    |--------------------------------------------------------------------------
    | Token Caching
    |--------------------------------------------------------------------------
    |
    | Configuration for caching OAuth2 access tokens. The SDK automatically
    | refreshes tokens before they expire.
    |
    */
    'token_cache' => [
        'driver' => env('FLOWCATALYST_TOKEN_CACHE_DRIVER', 'file'),
        'key' => env('FLOWCATALYST_TOKEN_CACHE_KEY', 'flowcatalyst_access_token'),
    ],

    /*
    |--------------------------------------------------------------------------
    | HTTP Client Settings
    |--------------------------------------------------------------------------
    |
    | Configuration for the HTTP client used to communicate with the
    | FlowCatalyst API.
    |
    */
    'http' => [
        'timeout' => env('FLOWCATALYST_TIMEOUT', 30),
        'retry_attempts' => env('FLOWCATALYST_RETRY_ATTEMPTS', 3),
        'retry_delay' => env('FLOWCATALYST_RETRY_DELAY', 100), // milliseconds
    ],

    /*
    |--------------------------------------------------------------------------
    | OIDC User Authentication
    |--------------------------------------------------------------------------
    |
    | Configuration for authenticating users via FlowCatalyst's OIDC server.
    | This is separate from the service account credentials above which are
    | used for API access.
    |
    | User authentication flow:
    | 1. User clicks login -> redirected to FlowCatalyst
    | 2. User authenticates -> redirected back with code
    | 3. Code exchanged for tokens -> your OidcUserHandler is called
    |
    | For API access to FlowCatalyst, use the service account credentials
    | (client_id/client_secret above with client_credentials grant).
    |
    */
    'oidc' => [
        /*
        |----------------------------------------------------------------------
        | Enable OIDC User Authentication
        |----------------------------------------------------------------------
        |
        | Set to false to disable the OIDC routes entirely.
        |
        */
        'enabled' => env('FLOWCATALYST_OIDC_ENABLED', false),

        /*
        |----------------------------------------------------------------------
        | Native Login Bridge (opt-in)
        |----------------------------------------------------------------------
        |
        | The bridge makes a FRESH app authenticate against FlowCatalyst with
        | (almost) zero code. It is OPT-IN — leave it off and `enabled` above
        | only registers the OIDC routes/guards; the SDK won't upsert users or
        | auto-redirect guests. Existing apps that wire their own login hook
        | (their own OidcUserHandler / auth middleware) should leave this OFF.
        |
        | When ON it changes two defaults (each still individually overridable):
        |   - the default `handler` becomes 'database' (upsert a local user +
        |     native Auth::login) instead of 'session';
        |   - `auto_guest_redirect` sends guests on `auth` routes into the
        |     FlowCatalyst login flow.
        |
        */
        'native_login' => env('FLOWCATALYST_NATIVE_LOGIN', false),

        /*
        |----------------------------------------------------------------------
        | OIDC Client ID (for user authentication)
        |----------------------------------------------------------------------
        |
        | The OAuth client ID for user authentication. This is different from
        | the service account client_id used for API access.
        |
        | Create an OAuth client in FlowCatalyst with:
        | - Grant types: authorization_code
        | - Redirect URI: https://your-app.com/flowcatalyst/callback
        |
        */
        'client_id' => env('FLOWCATALYST_OIDC_CLIENT_ID'),

        /*
        |----------------------------------------------------------------------
        | Provider-Direct Login (optional)
        |----------------------------------------------------------------------
        |
        | Identity provider id (idp_...) to send every login straight to a
        | named upstream IdP, bypassing the FlowCatalyst login page — the
        | portal pattern ("Login with Acme SSO"). A per-request ?provider=
        | query param on the login route overrides this default. Leave empty
        | for the standard FlowCatalyst login page.
        |
        */
        'provider' => env('FLOWCATALYST_OIDC_PROVIDER'),

        /*
        |----------------------------------------------------------------------
        | Branded Sign-In (optional)
        |----------------------------------------------------------------------
        |
        | FlowCatalyst client identifier (the URL-safe slug, e.g. "acme") whose
        | login branding the sign-in pages should wear — logo, colours, brand
        | name and footer, as configured under Clients -> Login Branding. A
        | per-request ?client= query param on the login route overrides this
        | default.
        |
        | This is purely cosmetic. It does not affect who may sign in or what
        | they may access, and an absent or unrecognised value simply falls
        | back to the platform-wide theme. Ignored in portal mode.
        |
        */
        'client' => env('FLOWCATALYST_OIDC_CLIENT'),

        /*
        |----------------------------------------------------------------------
        | Portal Mode (optional)
        |----------------------------------------------------------------------
        |
        | Set to true when this app is a PORTAL for a client's customers: the
        | login flow enters through FlowCatalyst's portal identity plane
        | (/portal/authorize) — a separate end-user population from platform
        | users, with per-client invites/SSO and no platform session reuse.
        | Requires the OAuth client to have its portal owner client set.
        | The callback and token exchange are unchanged.
        |
        */
        'portal' => env('FLOWCATALYST_OIDC_PORTAL', false),

        /*
        |----------------------------------------------------------------------
        | OIDC Client Secret (optional for public clients)
        |----------------------------------------------------------------------
        |
        | For confidential clients, provide the client secret.
        | For public clients (SPAs), leave empty and use PKCE only.
        |
        */
        'client_secret' => env('FLOWCATALYST_OIDC_CLIENT_SECRET'),

        /*
        |----------------------------------------------------------------------
        | Requested Scopes
        |----------------------------------------------------------------------
        |
        | The scopes to request during authentication.
        | Default: openid profile email
        |
        */
        'scope' => env('FLOWCATALYST_OIDC_SCOPE', 'openid profile email'),

        /*
        |----------------------------------------------------------------------
        | Route Configuration
        |----------------------------------------------------------------------
        |
        | Customize the routes used for OIDC authentication.
        |
        */
        'login_route' => env('FLOWCATALYST_OIDC_LOGIN_ROUTE', '/flowcatalyst/login'),
        'callback_route' => env('FLOWCATALYST_OIDC_CALLBACK_ROUTE', '/flowcatalyst/callback'),
        'logout_route' => env('FLOWCATALYST_OIDC_LOGOUT_ROUTE', '/flowcatalyst/logout'),
        'refresh_route' => env('FLOWCATALYST_OIDC_REFRESH_ROUTE', '/flowcatalyst/refresh'),

        /*
        |----------------------------------------------------------------------
        | Redirect URLs
        |----------------------------------------------------------------------
        |
        | Where to redirect after login/logout.
        | These can be overridden by implementing OidcUserHandler.
        |
        */
        'redirect_after_login' => env('FLOWCATALYST_REDIRECT_AFTER_LOGIN', '/dashboard'),
        'redirect_after_logout' => env('FLOWCATALYST_REDIRECT_AFTER_LOGOUT', '/'),
        'error_redirect' => env('FLOWCATALYST_ERROR_REDIRECT', '/'),

        /*
        |----------------------------------------------------------------------
        | OIDC Callback Handler
        |----------------------------------------------------------------------
        |
        | What happens on a successful OIDC callback:
        |   'database' — upsert a local user row and Auth::login() them into the
        |     native Laravel guard, so stock `->middleware('auth')` recognises
        |     them. Still stores the SDK session principal too.
        |   'session'  — store only the SDK session principal (no native
        |     Auth::login / no users row). Pair with `AUTH_GUARD=fc-session` to
        |     authenticate stock `auth` routes off the token principal.
        |
        | DEFAULT: unset → resolved from `native_login` above ('database' when
        | the bridge is on, otherwise 'session'). Set it explicitly to pin one.
        |
        | Apps that need custom mapping (tenant checks, extra columns, …) bind
        | their own OidcUserHandler — that binding always wins over this default.
        |
        */
        'handler' => env('FLOWCATALYST_OIDC_HANDLER'),

        /*
        | The Eloquent user model upserted + logged in by the 'database' handler.
        | Matched by email. Defaults to a standard Laravel app's user model.
        */
        'user_model' => env('FLOWCATALYST_OIDC_USER_MODEL', \App\Models\User::class),

        /*
        | Remember the native login (sets the long-lived "remember me" cookie).
        */
        'remember_login' => env('FLOWCATALYST_OIDC_REMEMBER_LOGIN', false),

        /*
        |----------------------------------------------------------------------
        | Role Syncing (Spatie laravel-permission)
        |----------------------------------------------------------------------
        |
        | When the user model uses Spatie's HasRoles trait, the 'database'
        | handler maps the token's `roles` claim onto the local user on every
        | login. No-op if spatie/laravel-permission isn't installed.
        |
        |   sync_roles          — master switch for role syncing on login.
        |   sync_roles_mode     — 'additive' grants new roles and never removes
        |                         existing ones; 'replace' makes the token's
        |                         roles the user's authoritative full set.
        |   create_missing_roles— create local roles that don't exist yet
        |                         (off by default: only assign roles you manage).
        |   roles_guard         — Spatie guard_name to match/create roles under.
        |
        */
        'sync_roles' => env('FLOWCATALYST_OIDC_SYNC_ROLES', true),
        'sync_roles_mode' => env('FLOWCATALYST_OIDC_SYNC_ROLES_MODE', 'additive'),
        'create_missing_roles' => env('FLOWCATALYST_OIDC_CREATE_MISSING_ROLES', false),
        'roles_guard' => env('FLOWCATALYST_OIDC_ROLES_GUARD', 'web'),

        /*
        | On login, also mirror each role's PERMISSIONS from FlowCatalyst into the
        | local Spatie role (so $user->can('app:ctx:agg:act') works), not just the
        | role name. Fetched via GET /api/roles/{role} using the service account
        | and cached per role for `role_permissions_cache_ttl` seconds. Best-effort:
        | skipped silently if no service account is configured / the API is down.
        */
        'sync_role_permissions' => env('FLOWCATALYST_OIDC_SYNC_ROLE_PERMISSIONS', true),
        'role_permissions_cache_ttl' => (int) env('FLOWCATALYST_OIDC_ROLE_PERMISSIONS_CACHE_TTL', 300),

        /*
        |----------------------------------------------------------------------
        | Permission Resolver (fc-token / fc-session guards)
        |----------------------------------------------------------------------
        |
        | How a token/session principal's permissions are resolved for
        | hasPermissionTo()/can(). Independent of the Spatie/users-table sync.
        |
        |   'token'  (default) — read straight off the token's `scope` claim.
        |                        Fully stateless: no HTTP, no DB, no Spatie.
        |   'api_me'            — server-resolved via GET /api/me (one cached
        |                        HTTP call; fresher when permissions change).
        |
        | A locally-bound RbacCatalogue (offline, from roles) overrides either.
        |
        */
        'permission_resolver' => env('FLOWCATALYST_OIDC_PERMISSION_RESOLVER', 'token'),

        /*
        |----------------------------------------------------------------------
        | Auto Guest Redirect
        |----------------------------------------------------------------------
        |
        | When enabled (and OIDC is enabled), unauthenticated requests to routes
        | guarded by the stock `auth` middleware are redirected straight into the
        | FlowCatalyst login flow — so you never have to define a `login` route
        | or a custom redirect. If your app already has its own `login` route,
        | that route is preferred and this does nothing (it won't hijack a local
        | login page). Set to false to opt out entirely.
        |
        */
        'auto_guest_redirect' => env('FLOWCATALYST_OIDC_AUTO_GUEST_REDIRECT', true),

        /*
        |----------------------------------------------------------------------
        | Single Logout (RP-Initiated Logout)
        |----------------------------------------------------------------------
        |
        | When true, logout redirects through FlowCatalyst's end-session
        | endpoint so the user is signed out of the IdP too — not just this
        | app. The `redirect_after_logout` URL is sent as post_logout_redirect_uri
        | and MUST be registered in this client's `postLogoutRedirectUris`
        | whitelist on the platform (exact, or a subdomain wildcard such as
        | https://*.inhanceapps.com). Defaults to off to preserve local-only
        | logout behaviour.
        |
        */
        'single_logout' => env('FLOWCATALYST_OIDC_SINGLE_LOGOUT', false),

        /*
        |----------------------------------------------------------------------
        | Service-token fallback middleware
        |----------------------------------------------------------------------
        |
        | Used by the `fc.or-passport` middleware: a request that is NOT a valid
        | FlowCatalyst access token is handed to this middleware chain instead
        | (e.g. your existing Passport client-credentials guard). FlowCatalyst
        | tokens never touch it. Leave empty for FlowCatalyst-only routes.
        |
        */
        'token_fallback_middleware' => [
            // 'client', 'passport.app.identity',
        ],

        /*
        |----------------------------------------------------------------------
        | Service-token permission resolution (GET /api/me)
        |----------------------------------------------------------------------
        |
        | The token guard fetches the caller's server-resolved roles +
        | permissions from /api/me and caches them keyed by the access token for
        | this many seconds (one HTTP call per token, then cached). A short TTL
        | keeps revocation/role changes responsive. If /api/me is unreachable
        | the guard falls back to a bound RbacCatalogue (if any).
        |
        */
        'me_cache_ttl_seconds' => (int) env('FLOWCATALYST_OIDC_ME_CACHE_TTL', 60),
        'me_cache_driver' => env('FLOWCATALYST_OIDC_ME_CACHE_DRIVER'),

        /*
        |----------------------------------------------------------------------
        | Session refresh leeway
        |----------------------------------------------------------------------
        |
        | A session principal is capped to its OWN access token's real expiry
        | (not the short-lived ID token, not Laravel's session lifetime) —
        | AuthenticateFc silently refreshes it via the refresh_token this many
        | seconds BEFORE that expiry, so a request landing right at the
        | boundary doesn't get caught with an already-expired token.
        |
        */
        'session_refresh_leeway_seconds' => (int) env('FLOWCATALYST_OIDC_SESSION_REFRESH_LEEWAY', 30),

        /*
        |----------------------------------------------------------------------
        | Revocation check (opt-in)
        |----------------------------------------------------------------------
        |
        | When enabled, AuthenticateFc additionally asks
        | GET /api/principals/{id}/version whether the principal (or a role it
        | holds) changed since the stored access token was minted, and forces
        | an early refresh if so — catching a revoked role/deactivated user
        | faster than waiting for the access token's natural (up to ~1 hour)
        | expiry. Off by default: it's an extra HTTP call per request (bounded
        | by the cache below) for apps that need tighter revocation than the
        | mandatory expiry-based cap already provides.
        |
        | revocation_cache_ttl_seconds bounds how often that HTTP call
        | actually happens — a request only re-asks the platform once this
        | many seconds have passed since the last check for that principal;
        | in between, the previous answer is reused. Clamped to [5, 600]
        | regardless of what's configured here, so a stray 0 can't hammer
        | the platform on every request and a stray huge value can't quietly
        | make this opt-in feature pointless.
        |
        */
        'check_revocation' => env('FLOWCATALYST_OIDC_CHECK_REVOCATION', false),
        'revocation_cache_ttl_seconds' => (int) env('FLOWCATALYST_OIDC_REVOCATION_CACHE_TTL', 60),
        'revocation_cache_driver' => env('FLOWCATALYST_OIDC_REVOCATION_CACHE_DRIVER'),

        /*
        |----------------------------------------------------------------------
        | Route Middleware
        |----------------------------------------------------------------------
        |
        | Middleware to apply to the OIDC routes. The 'web' middleware is
        | required for session handling (PKCE state storage).
        |
        | Note: Auth middleware (auth, auth:sanctum, etc.) is automatically
        | excluded from these routes since they ARE the authentication
        | mechanism. If you have custom auth middleware causing issues,
        | add it to 'exclude_middleware' below.
        |
        */
        'middleware' => ['web'],

        /*
        |----------------------------------------------------------------------
        | Excluded Middleware
        |----------------------------------------------------------------------
        |
        | Additional middleware to exclude from the OIDC routes. The SDK
        | automatically excludes: auth, auth:sanctum, auth:api, auth:web.
        |
        | Add any custom auth middleware here that should not run on the
        | login/callback/logout routes.
        |
        */
        'exclude_middleware' => [],
    ],

    /*
    |--------------------------------------------------------------------------
    | Application Code (Single-App Deployments)
    |--------------------------------------------------------------------------
    |
    | The unique code for your application in FlowCatalyst. Used by the
    | `flowcatalyst:sync` artisan command when syncing definitions.
    |
    | This is the DEFAULT application code. For a codebase that defines
    | definitions for MORE THAN ONE application, you don't have to drop to the
    | programmatic API — `flowcatalyst:sync` resolves each definition's app per
    | this order and syncs each application separately:
    |
    |   1. an explicit `application:` on the attribute (e.g. #[AsEventType]),
    |   2. a namespace match in `definitions.application_map` below,
    |   3. this default.
    |
    */
    'application_code' => env('FLOWCATALYST_APP_CODE'),

    /*
    |--------------------------------------------------------------------------
    | Client (Single-Tenant Deployments)
    |--------------------------------------------------------------------------
    |
    | The FlowCatalyst client (by IDENTIFIER slug, never an id — ids differ
    | per environment) that #[AsSubscription] / #[AsConnection] definitions
    | belong to when the attribute itself doesn't set `client:`. Leave unset
    | (the default) for a global — client-less — definition.
    |
    | This is only for SINGLE-tenant apps. A MULTI-tenant application (one
    | codebase, many clients) doesn't use this default at all — it builds one
    | `SyncDefinitionSet` per client programmatically instead; see
    | `definitions.set_providers` below and docs/syncing-definitions.md.
    |
    */
    'client' => env('FLOWCATALYST_CLIENT'),

    /*
    |--------------------------------------------------------------------------
    | Definition Scanning
    |--------------------------------------------------------------------------
    |
    | Configuration for scanning PHP classes with FlowCatalyst attributes
    | (#[AsRole], #[AsEventType], #[AsSubscription], #[AsConnection]) and
    | caching them for syncing to the platform.
    |
    */
    'definitions' => [
        /*
        |----------------------------------------------------------------------
        | Application Map (Multi-Application Codebases)
        |----------------------------------------------------------------------
        |
        | Maps a class-namespace PREFIX to a FlowCatalyst application code. When
        | a scanned definition's class falls under a prefix, it syncs to that
        | application instead of the default `application_code`. Longest prefix
        | wins; an explicit `application:` on the attribute beats the map.
        |
        | Use this when one codebase owns multiple apps, or when a package ships
        | its own definitions and is its own application — the CONSUMER maps the
        | package's namespace to whatever app code they registered:
        |
        |   'application_map' => [
        |       'App\\Orders\\'           => 'orders',
        |       'Vendor\\Logistics\\'     => 'logistics',
        |   ],
        |
        */
        'application_map' => [],

        /*
        |----------------------------------------------------------------------
        | Definition Set Providers (Multi-Tenant Deployments)
        |----------------------------------------------------------------------
        |
        | Class names implementing `FlowCatalyst\Sync\ProvidesSyncDefinitionSets`,
        | resolved through the container. A multi-tenant application's client
        | list is runtime data (a database table, a config file, …) that no
        | attribute could express, so it builds one `SyncDefinitionSet` per
        | (application, client) programmatically here instead of using
        | attributes for this. `flowcatalyst:sync` resolves each provider and
        | syncs every set it yields, IN ADDITION to the scanned attribute
        | definitions:
        |
        |   'set_providers' => [
        |       App\FlowCatalyst\TenantDefinitionSetProvider::class,
        |   ],
        |
        */
        'set_providers' => [],

        /*
        |----------------------------------------------------------------------
        | Scan Paths
        |----------------------------------------------------------------------
        |
        | Directories to scan for FlowCatalyst definition classes. These are
        | PHP classes with attributes like #[AsRole], #[AsEventType], or
        | #[AsSubscription].
        |
        | Default: app/FlowCatalyst
        |
        */
        'paths' => [
            app_path('FlowCatalyst'),
        ],

        /*
        |----------------------------------------------------------------------
        | Cache Path
        |----------------------------------------------------------------------
        |
        | Directory where scanned definitions are cached as JSON. The cache
        | is used by the sync command to avoid rescanning on every sync.
        |
        | Default: storage/flowcatalyst
        |
        */
        'cache_path' => storage_path('flowcatalyst'),

        /*
        |----------------------------------------------------------------------
        | Seed Spatie on Sync
        |----------------------------------------------------------------------
        |
        | When true, `php artisan flowcatalyst:sync` also mirrors the scanned
        | roles + permissions into the local spatie/laravel-permission tables
        | (under flowcatalyst.oidc.roles_guard), so the app's local authorization
        | model matches what it pushes to the platform. No-op if Spatie isn't
        | installed. Pass --no-spatie to skip for a single run.
        |
        */
        'seed_spatie' => env('FLOWCATALYST_SEED_SPATIE', true),
    ],

    /*
    |--------------------------------------------------------------------------
    | Scheduled Jobs
    |--------------------------------------------------------------------------
    |
    | The SDK mounts the scheduled-job process endpoint (POST
    | /api/_fc/scheduled-jobs/process) automatically and, on sync, defaults
    | each job's targetUrl to APP_URL + that path when the #[AsScheduledJob]
    | attribute doesn't set one — so firings work with zero route wiring.
    |
    */
    'scheduled_jobs' => [
        // Set false to mount your own route to ScheduledJobRunner
        // ::processWithResponse() (e.g. under extra middleware). If you do,
        // set process_url (or per-job targetUrl) to match, and guard your
        // route with the `fc-signature` middleware — the platform signs every
        // firing with your application service account's signing secret, and
        // validation FAILS CLOSED when FLOWCATALYST_SIGNING_SECRET is unset.
        // The signature covers the raw request body: keep the route out of
        // body-mutating middleware groups.
        'register_route' => env('FLOWCATALYST_SCHEDULED_JOBS_ROUTE', true),

        // Full URL the platform POSTs firings to. Overrides the APP_URL-based
        // default — useful when the externally reachable URL differs from
        // APP_URL (tunnels, internal gateways).
        'process_url' => env('FLOWCATALYST_SCHEDULED_JOBS_PROCESS_URL'),

        // Auto-wire handlers from scanned #[AsScheduledJob] classes that have
        // a handle(array $envelope, callable $log) method (the
        // HandlesScheduledJob contract) — apps then ship zero HTTP code for
        // scheduled jobs. Manual $runner->handler() registrations always win.
        'auto_handlers' => env('FLOWCATALYST_SCHEDULED_JOBS_AUTO_HANDLERS', true),
    ],

    /*
    |--------------------------------------------------------------------------
    | Subscriptions
    |--------------------------------------------------------------------------
    |
    | #[AsSubscription(target: ...)] takes either an absolute URL or a path.
    | A path is resolved at SYNC time against target_base_url, which defaults
    | to APP_URL — so one definition serves every environment. Set it only
    | when the URL the platform must call differs from APP_URL (tunnels,
    | internal gateways, a per-tenant host).
    |
    */
    'subscriptions' => [
        'target_base_url' => env('FLOWCATALYST_SUBSCRIPTION_TARGET_BASE_URL'),
    ],

    /*
    |--------------------------------------------------------------------------
    | Outbox Configuration
    |--------------------------------------------------------------------------
    |
    | The outbox allows your application to write events and dispatch jobs
    | directly to a local database table, implementing the transactional
    | outbox pattern. The outbox processor will poll this table and send
    | messages to FlowCatalyst.
    |
    */
    'outbox' => [
        /*
        |----------------------------------------------------------------------
        | Enable Outbox
        |----------------------------------------------------------------------
        |
        | Set to false to disable the outbox functionality entirely.
        |
        */
        'enabled' => env('FLOWCATALYST_OUTBOX_ENABLED', true),

        /*
        |----------------------------------------------------------------------
        | Outbox Driver
        |----------------------------------------------------------------------
        |
        | The driver to use for storing outbox messages.
        | Supported: "database" (MySQL 8.0+, PostgreSQL 12+), "mongodb"
        |
        */
        'driver' => env('FLOWCATALYST_OUTBOX_DRIVER', 'database'),

        /*
        |----------------------------------------------------------------------
        | Database Connection
        |----------------------------------------------------------------------
        |
        | The database connection to use for the outbox. Leave null to use
        | the default connection.
        |
        */
        'connection' => env('FLOWCATALYST_OUTBOX_CONNECTION'),

        /*
        |----------------------------------------------------------------------
        | Table/Collection Name
        |----------------------------------------------------------------------
        |
        | The name of the table (or MongoDB collection) for outbox messages.
        |
        */
        'table' => env('FLOWCATALYST_OUTBOX_TABLE', 'outbox_messages'),

        /*
        |----------------------------------------------------------------------
        | Tenant ID
        |----------------------------------------------------------------------
        |
        | Your FlowCatalyst tenant ID. This is required for the outbox to
        | function correctly.
        |
        */
        'tenant_id' => env('FLOWCATALYST_TENANT_ID'),

        /*
        |----------------------------------------------------------------------
        | Audit Logging
        |----------------------------------------------------------------------
        |
        | When enabled, the use-case envelope (Operation + Runner / UnitOfWork)
        | writes an audit-log row to the outbox alongside each domain event it
        | commits — forwarded to the platform's audit log.
        |
        */
        'audit_enabled' => env('FLOWCATALYST_OUTBOX_AUDIT_ENABLED', false),

        /*
        |----------------------------------------------------------------------
        | Client Code (Client-Centric Linkage)
        |----------------------------------------------------------------------
        |
        | The FlowCatalyst client (by CODE) that events + audit logs emitted by
        | the use-case envelope belong to. The platform is client-centric and
        | resolves this code → client_id at ingest (events) / and the audit log
        | carries it alongside the application code. Leave empty to omit.
        |
        | This is distinct from `outbox.client_id` above, which is the owner of
        | the local outbox ROWS, not the business client the data belongs to.
        |
        */
        'client_code' => env('FLOWCATALYST_CLIENT_CODE'),

        /*
        |----------------------------------------------------------------------
        | Outbox Client
        |----------------------------------------------------------------------
        |
        | The FlowCatalyst client that owns outbox rows (the `client_id` column).
        | Defaults to the top-level `client_id` (FLOWCATALYST_CLIENT_ID) when
        | unset. The legacy `tenant_id` above is no longer used by the outbox.
        |
        */
        'client_id' => env('FLOWCATALYST_OUTBOX_CLIENT_ID'),

        /*
        |----------------------------------------------------------------------
        | Default Partition
        |----------------------------------------------------------------------
        |
        | The default partition ID to use when none is specified.
        |
        */
        'default_partition' => env('FLOWCATALYST_DEFAULT_PARTITION', 'default'),

        /*
        |----------------------------------------------------------------------
        | Strict Transactional Outbox
        |----------------------------------------------------------------------
        |
        | When enabled, the database outbox driver throws an OutboxException
        | if a write is attempted outside of an active database transaction
        | on the configured connection. This catches the most common
        | transactional-outbox bug: writing the outbox row without wrapping
        | it in `DB::transaction(fn () => ...)` alongside the business writes,
        | which lets business state and outbox state drift on partial failure.
        |
        | When disabled (default, for backwards compatibility), a warning is
        | logged via Log::warning(...) but the write proceeds. Flip this on
        | per environment once your callers consistently wrap outbox writes
        | in DB::transaction(...).
        |
        */
        'strict_transactions' => env('FLOWCATALYST_OUTBOX_STRICT_TRANSACTIONS', false),
    ],
];
