# FlowCatalyst Laravel SDK

Official Laravel SDK for the [FlowCatalyst](https://flowcatalyst.io) platform.

Use FlowCatalyst as your app's **identity provider and authorization layer**, and
talk to the **event-driven control plane** — all from idiomatic Laravel.

**What's in the box**

- **Sign in with FlowCatalyst (OIDC)** — stock `->middleware('auth')` drives the
  login flow; guests are redirected, users come back authenticated. No scaffolding.
- **Authorization that feels like Spatie** — `$user->can(...)`, `@can`, `hasRole()`,
  policies — backed by FlowCatalyst roles/permissions. Run it **stateless** (no
  users table, no Spatie) or sync into a local `users` table + `spatie/laravel-permission`.
- **API auth** — validate `client_credentials` / user Bearer tokens with the same
  permission surface (`auth:fc-token`).
- **Declare & sync your RBAC** — define roles and permissions as PHP classes
  (`#[AsRole]`, `#[AsPermission]`) and push them to the platform.
- **Outbox** — create events/dispatch jobs via the transactional outbox.
- **Control plane client** — typed access to event types, subscriptions, dispatch
  pools, applications, roles, and more.
- **Webhook validation** — verify inbound HMAC-signed deliveries.

## Requirements

- PHP 8.1+
- Laravel 10, 11, 12, or 13
- For the Outbox: MySQL 8.0+ / PostgreSQL 12+ / MongoDB 4.4+
- For local authorization mirroring (optional): `spatie/laravel-permission`

## Contents

- [Installation](#installation) · [Local dev with `fcdev`](#local-development-with-fcdev)
- [Authentication & authorization](#authentication--authorization) — login, guards, permissions, modes, refresh
- [Declaring & syncing roles and permissions](#declaring--syncing-roles-and-permissions)
- [Control plane API](#control-plane-api) · [Portal identities](#portal-identities) · [Outbox](#outbox-event-creation) · [Webhooks](#webhook-validation)

## Installation

```bash
composer require flowcatalyst/laravel-sdk
```

Publish the configuration file:

```bash
php artisan vendor:publish --tag=flowcatalyst-config
```

## Local development with `fcdev`

For local work you need a FlowCatalyst control plane to talk to.
`fcdev` is the official one-binary dev environment — bundled
PostgreSQL, platform API, message router, scheduler, and frontend
in a single process.

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/flowcatalyst/flowcatalyst/main/install.sh | sh

# Windows (PowerShell)
irm https://raw.githubusercontent.com/flowcatalyst/flowcatalyst/main/install.ps1 | iex

fcdev          # starts API on http://localhost:8080
```

If you use the **Outbox** (outbox pattern), you also need
`fcdev outbox` running as a sidecar — the SDK only writes rows to
your app's `outbox_messages` table; `fcdev outbox poll` is what
forwards them to the platform.

```bash
# In your Laravel project directory:

# Once: write FC_OUTBOX_DB_URL / FC_OUTBOX_API_URL / FC_OUTBOX_TOKEN
# into ./.env (0600 perms; no secrets on argv or shell history).
# fcdev outbox init appends to your existing .env — your other
# FLOWCATALYST_* keys are not touched.
fcdev outbox init

# Daily: reads .env, auto-creates the `outbox_messages` table on
# first run if your outbox migration hasn't been run yet, then polls.
fcdev outbox poll
```

Complete reference: run `fcdev --help`, or see the
[FlowCatalyst repository](https://github.com/flowcatalyst/flowcatalyst).

## Configuration

Add the following to your `.env` file:

```env
# FlowCatalyst API
FLOWCATALYST_BASE_URL=https://your-instance.flowcatalyst.io
FLOWCATALYST_CLIENT_ID=your_client_id
FLOWCATALYST_CLIENT_SECRET=your_client_secret

# Outbox (for event creation)
FLOWCATALYST_OUTBOX_DRIVER=database

# Webhook validation (optional)
FLOWCATALYST_SIGNING_SECRET=your_signing_secret
```

## Authentication & authorization

The SDK does two things: **signs users in** against FlowCatalyst's OIDC server,
and lets you **authorize** them with a Spatie-style surface (`can`, `@can`,
`hasRole`, policies) backed by FlowCatalyst roles/permissions.

### Quick start — sign in with FlowCatalyst

A fresh Laravel app needs only env. Stock `->middleware('auth')`, `Auth::user()`,
`@auth`, and policies all recognise the signed-in user.

```env
FLOWCATALYST_BASE_URL=https://your-instance.flowcatalyst.io
FLOWCATALYST_OIDC_ENABLED=true
FLOWCATALYST_NATIVE_LOGIN=true   # the zero-code bridge: local-user upsert + guest redirect
FLOWCATALYST_OIDC_CLIENT_ID=your_oauth_client_id
FLOWCATALYST_OIDC_CLIENT_SECRET=your_oauth_client_secret   # omit for a PKCE-only public client
FLOWCATALYST_OIDC_SCOPE="openid profile email offline_access"   # offline_access → refresh tokens
```

Protect routes the normal Laravel way:

```php
// routes/web.php
Route::middleware('auth')->group(function () {
    Route::get('/dashboard', DashboardController::class);
});
```

That's it. With the native-login bridge on (`FLOWCATALYST_NATIVE_LOGIN=true` —
it's opt-in so the SDK never changes an existing app's auth behaviour), a guest
hitting an `auth` route is **automatically redirected into the FlowCatalyst
login flow** — no `login` route or custom redirect needed. (If your app defines
its own `login` route, that one wins; the SDK won't hijack a local login page.
Opt out of just the redirect with `FLOWCATALYST_OIDC_AUTO_GUEST_REDIRECT=false`.)

The routes `/flowcatalyst/{login,callback,logout,refresh}` are registered for
you. On your FlowCatalyst OAuth client, set the grant to `authorization_code`
and the redirect URI to **`https://your-app.com/flowcatalyst/callback`** (it must
match exactly — host, port, and scheme — including `localhost` vs `127.0.0.1`).

### Checking permissions

Once authenticated, the user (whether resolved from a browser session or a
Bearer token) speaks the Spatie vocabulary — FlowCatalyst wildcards honoured:

```php
$user->hasPermissionTo('orders:admin:order:cancel');
$user->hasRole('orders:admin');
$user->can('orders:admin:order:cancel');           // Gate, @can, policies route here

// Blade
@can('orders:admin:order:cancel')
    <button>Cancel order</button>
@endcan
```

Permission strings are 4-part `application:context:aggregate:action`; role names
are platform-prefixed (`orders:admin`).

### Two modes

| | **Stateless** (default) | **Local sync** |
|---|---|---|
| Local `users` row | none | upserted on login |
| `spatie/laravel-permission` | not needed | used (roles/permissions mirrored) |
| Permissions come from | the token's `scope` claim | local Spatie tables |
| `Auth::user()` guard | `fc-session` / `fc-token` | native `web` |
| Enable | `FLOWCATALYST_OIDC_HANDLER=session` | `FLOWCATALYST_OIDC_HANDLER=database` |

**Stateless** is the leanest: no migrations, no Spatie, nothing to keep in sync —
the access token already carries the granted permissions. **Local sync** is for
apps that want a real `users` row (to foreign-key against) and to manage
authorization through Spatie locally.

#### Stateless (no users table, no Spatie)

```env
FLOWCATALYST_OIDC_HANDLER=session
FLOWCATALYST_OIDC_PERMISSION_RESOLVER=token   # read permissions off the token scope (default)
```

Authorize via the FlowCatalyst guards:

- **Browser / OIDC login:** `->middleware('auth:fc-session')` (or set `fc-session`
  as your app's default guard so bare `auth` uses it).
- **API / Bearer** (user *and* `client_credentials` tokens): `->middleware('auth:fc-token')`.

Both resolve a `FlowCatalystAuthenticatable` — read-only, no DB. Set
`FLOWCATALYST_OIDC_PERMISSION_RESOLVER=api_me` to resolve via `/api/me` instead
(one cached HTTP call, fresher on changes), or bind an offline `RbacCatalogue`.

> **Policies: don't type-hint your Eloquent user.** `FlowCatalystAuthenticatable`
> is not `App\Models\User`, so a policy method signed
> `view(User $user, Order $order)` never matches a token/session principal —
> Laravel skips the method and the check **silently denies**. Type-hint the
> contract (or nothing) in policies that must run for FlowCatalyst identities:
>
> ```php
> public function view(Authenticatable $user, Order $order): bool
> {
>     return $user->hasPermissionTo('orders:admin:order:view');
> }
> ```
>
> (In **local-sync** mode this doesn't apply — there `Auth::user()` IS your
> Eloquent model.) Spatie's own `role:`/`permission:` route middleware work
> unchanged with either mode.

#### Local sync (users table + Spatie)

```env
FLOWCATALYST_OIDC_HANDLER=database
FLOWCATALYST_OIDC_USER_MODEL="App\Models\User"   # upserted by email on login
```

On login the SDK upserts the user, `Auth::login()`s them, and — if the model
uses Spatie's `HasRoles` trait — mirrors the token's roles **and each role's
permissions** into the local Spatie tables, so stock `auth` + `$user->can(...)`
work against a real record:

```env
FLOWCATALYST_OIDC_SYNC_ROLES=true
FLOWCATALYST_OIDC_SYNC_ROLES_MODE=additive          # or "replace" for authoritative
FLOWCATALYST_OIDC_SYNC_ROLE_PERMISSIONS=true        # pull each role's permissions too
FLOWCATALYST_OIDC_CREATE_MISSING_ROLES=false        # create local roles on the fly
FLOWCATALYST_OIDC_ROLES_GUARD=web
```

### Middleware reference

The service provider registers these aliases (no app wiring needed):

| Middleware | Behaviour |
|---|---|
| `auth:fc-token` | Authenticate a FlowCatalyst Bearer token (user or `client_credentials`); 401 otherwise. Stack guards to fall through: `auth:fc-token,api`. |
| `auth:fc-session` | Authenticate the OIDC session principal; 401/redirect otherwise. |
| `fc.auth` | Resolve the principal (bearer wins over session) into `$request->attributes['fc.principal']` — never rejects. The three guards below only read that attribute, so they go **after** it: `['fc.auth', 'fc.bearer']`. |
| `fc.bearer` | 401 JSON when nothing resolved. For API routes. |
| `fc.session` | Redirect to `/flowcatalyst/login?returnTo=…` when nothing resolved. For web routes. |
| `fc.any` | Either mechanism; on miss, browser navigations redirect to login, everything else gets 401 JSON. |
| `fc.or-passport` | FlowCatalyst token **or** a legacy fallback chain (below). |

#### Migrating from Passport client-credentials

`fc.or-passport` lets one route accept FlowCatalyst service-account tokens
while still honouring your existing Passport machine tokens. A valid
FlowCatalyst token wins (the fallback never runs); anything else flows through
the configured chain exactly as if the middleware weren't there:

```php
// config/flowcatalyst.php
'oidc' => [
    'token_fallback_middleware' => ['client', 'passport.app.identity'],
],

Route::post('/api/stock-sync', $handler)->middleware('fc.or-passport');
```

An empty list means "FlowCatalyst-only": unauthenticated requests pass through
untouched, so a downstream guard decides. Once the last Passport caller is
migrated, swap the route to `auth:fc-token` and drop the fallback config.

### Keeping permissions fresh

In stateless mode permissions live in the (short-lived) access token, so they go
stale when roles change on the platform. `POST /flowcatalyst/refresh`
(`route('flowcatalyst.refresh')`) uses the stored refresh token to mint a fresh
access token and re-store the principal — wire it to a "Refresh" button:

```blade
<form method="POST" action="{{ route('flowcatalyst.refresh') }}">
    @csrf
    <button type="submit">Refresh permissions</button>
</form>
```

Requires a refresh token, so request `offline_access` in `FLOWCATALYST_OIDC_SCOPE`.
In local-sync mode, refresh also re-runs the role/permission sync.

### Configuration reference

| Env var | Default | Purpose |
|---|---|---|
| `FLOWCATALYST_OIDC_ENABLED` | `false` | turn the OIDC login + routes on |
| `FLOWCATALYST_NATIVE_LOGIN` | `false` | opt-in zero-code bridge: defaults the handler to `database` and enables the guest redirect |
| `FLOWCATALYST_OIDC_CLIENT_ID` / `_SECRET` | – | your OAuth login client (secret optional for PKCE) |
| `FLOWCATALYST_OIDC_SCOPE` | `openid profile email` | add `offline_access` for refresh tokens |
| `FLOWCATALYST_OIDC_HANDLER` | from bridge | `database` (users table) or `session` (stateless); unset → `database` when the bridge is on, else `session` |
| `FLOWCATALYST_OIDC_PERMISSION_RESOLVER` | `token` | `token` (scope claim) or `api_me` (`/api/me`) |
| `FLOWCATALYST_OIDC_AUTO_GUEST_REDIRECT` | `true` | redirect stock-`auth` guests into OIDC (only active with the bridge on) |
| `FLOWCATALYST_OIDC_PROVIDER` | – | `idp_...` id: send logins straight to a named upstream IdP (`?provider=` overrides per request) |
| `FLOWCATALYST_OIDC_CLIENT` | – | client identifier (slug) whose login branding the sign-in pages wear (`?client=` overrides per request); cosmetic only, ignored in portal mode |
| `FLOWCATALYST_OIDC_PORTAL` | `false` | portal mode: log in through `/portal/authorize` (see [Portal identities](#portal-identities)) |
| `FLOWCATALYST_OIDC_USER_MODEL` | `App\Models\User` | model upserted in `database` mode |
| `FLOWCATALYST_OIDC_SYNC_ROLES` / `_SYNC_ROLE_PERMISSIONS` | `true` | mirror roles / their permissions to Spatie |
| `FLOWCATALYST_OIDC_ROLES_GUARD` | `web` | Spatie guard for synced roles |
| `FLOWCATALYST_REDIRECT_AFTER_LOGIN` | `/dashboard` | post-login landing |

### Custom mapping

Need tenant checks, extra columns, or your own logic? Bind your own handler — it
always wins over the SDK default:

```php
// AppServiceProvider::register()
$this->app->bind(\FlowCatalyst\Auth\Contracts\OidcUserHandler::class, MyHandler::class);
```

Extend `DatabaseOidcUserHandler` to keep the upsert + login and override only
what you need, or implement `OidcUserHandler` from scratch.

The `DatabaseOidcUserHandler` + Spatie seeding/sync (`flowcatalyst:sync`,
`oidc.sync_role_permissions`) remain available for apps that *do* want a local
users table — when enabled, refresh re-runs the sync too.

## Declaring & syncing roles and permissions

Define your app's RBAC as PHP classes and push it to the platform — your repo is
the source of truth. FlowCatalyst has no standalone "create permission" endpoint,
so permissions reach the platform via the roles that grant them.

**Define** roles and reusable permissions in `app/FlowCatalyst`:

```php
// app/FlowCatalyst/Permissions/ViewPosts.php
use FlowCatalyst\Attributes\AsPermission;

// the application segment defaults to FLOWCATALYST_APP_CODE → "<app>:posts:post:view"
#[AsPermission(context: 'posts', aggregate: 'post', action: 'view', description: 'View posts')]
class ViewPosts {}
```

```php
// app/FlowCatalyst/Roles/EditorRole.php
use App\FlowCatalyst\Permissions\{EditPosts, ViewPosts};
use FlowCatalyst\Attributes\AsRole;

#[AsRole(name: 'editor', displayName: 'Editor', permissions: [ViewPosts::class, EditPosts::class])]
class EditorRole {}
```

A permission is declared once and **linked from many roles** by class. You can
also inline `new PermissionInput(...)` objects or plain `app:context:aggregate:action`
strings.

**Declare event types** by attributing your domain events, so the platform knows
about them (and they can be bound to subscribers and replayed):

```php
// app/UseCases/Comments/CommentCreated.php
use FlowCatalyst\Attributes\AsEventType;

// Synced code = "<app>:comments:comment:created" (the app prefix is added at sync).
// clientScoped: events of this type belong to a client.
#[AsEventType(subdomain: 'comments', aggregate: 'comment', event: 'created', name: 'Comment Created', clientScoped: true)]
final class CommentCreated extends BaseDomainEvent { /* … */ }
```

Make sure the directory holding them is listed in `flowcatalyst.definitions.paths`
(e.g. add `app_path('UseCases')` alongside the default `app_path('FlowCatalyst')`).

**Sync** with a service account (separate from your login client) that has
roles-write on the application:

```env
FLOWCATALYST_CLIENT_ID=svc_...        # client_credentials creds
FLOWCATALYST_CLIENT_SECRET=...
FLOWCATALYST_APP_CODE=blog            # the application that owns these roles
```

```bash
php artisan flowcatalyst:scan            # find #[AsRole]/#[AsPermission] classes, cache them
php artisan flowcatalyst:sync --dry-run  # preview
php artisan flowcatalyst:sync            # push roles (with their permissions) to the platform
```

If `spatie/laravel-permission` is installed, `flowcatalyst:sync` **also seeds the
local Spatie tables** so your app's authorization matches what it pushed (toggle
with `FLOWCATALYST_SEED_SPATIE` / `--no-spatie`). The same attribute mechanism
covers `#[AsEventType]`, `#[AsSubscription]`, `#[AsConnection]`, `#[AsDispatchPool]`,
`#[AsProcess]`, and `#[AsScheduledJob]`. See [`docs/syncing-definitions.md`](docs/syncing-definitions.md)
for the full reference, including the programmatic `DefinitionSynchronizer`.

### Multiple applications in one codebase

`flowcatalyst:sync` resolves each definition's application and syncs each app
separately — you don't have to drop to the programmatic API. The application is
resolved in this order:

1. an explicit `application:` on the attribute — `#[AsScheduledJob(application: 'orders', …)]`.
   Every definition attribute accepts it;
2. a longest-prefix match in the `definitions.application_map` config;
3. the default `--app` / `FLOWCATALYST_APP_CODE`.

The **map** is ideal when one codebase owns several apps, or when a package ships
its own definitions and is its own application — the *consumer* maps the package's
namespace to whatever app code they registered (a package author can't hardcode it):

```php
// config/flowcatalyst.php → definitions
'application_map' => [
    'App\\Orders\\'       => 'orders',
    'Vendor\\Logistics\\' => 'logistics',   // a package = its own application
],
```

`scan` records the resolved app per definition; `sync` groups by it and pushes
each application's catalog to its own `…/applications/{appCode}/…/sync` endpoint.

Note that `--remove-unlisted` is full-replace *per application, per category*,
and an application is only contacted when at least one definition resolves to
it. Moving the last definition of a category out of an app therefore leaves the
old rows in place rather than pruning them — see
[`docs/syncing-definitions.md`](docs/syncing-definitions.md).

## Control Plane API

The SDK provides typed, lower-level access to the control plane using
`client_credentials` (service account) authentication — for reads, ad-hoc
management, and anything not covered by the attribute sync above.

### Event Types

```php
use FlowCatalyst\Facades\FlowCatalyst;
use FlowCatalyst\DTOs\Requests\AddSchemaVersionRequest;
use FlowCatalyst\DTOs\Requests\CreateEventTypeRequest;

// List event types
$result = FlowCatalyst::eventTypes()->list();
foreach ($result->items as $eventType) {
    echo $eventType->code . ': ' . $eventType->name;
}

// Create an event type (returns the new id)
$eventTypeId = FlowCatalyst::eventTypes()->create(new CreateEventTypeRequest(
    code: 'order:fulfillment:order:created',
    name: 'Order Created',
    description: 'Fired when a new order is placed',
));

// Add a schema version
FlowCatalyst::eventTypes()->addSchemaVersion($eventTypeId, new AddSchemaVersionRequest(
    schema: ['type' => 'object', 'properties' => ['orderId' => ['type' => 'string']]],
    version: '1.0',
));
```

### Subscriptions

```php
use FlowCatalyst\Facades\FlowCatalyst;
use FlowCatalyst\DTOs\EventTypeBinding;
use FlowCatalyst\DTOs\Requests\CreateSubscriptionRequest;
use FlowCatalyst\Enums\DispatchMode;

// Create a subscription (returns the new id)
$subscriptionId = FlowCatalyst::subscriptions()->create(new CreateSubscriptionRequest(
    code: 'notify-warehouse',
    name: 'Notify Warehouse',
    endpoint: 'https://warehouse.example.com/webhook',
    eventTypes: [new EventTypeBinding('order:fulfillment:order:created')],
    dispatchPoolId: $poolId,
    mode: DispatchMode::IMMEDIATE,
    timeoutSeconds: 30,
    maxRetries: 5,
));

// Pause/resume a subscription
FlowCatalyst::subscriptions()->pause($subscriptionId);
FlowCatalyst::subscriptions()->resume($subscriptionId);
```

### Dispatch Pools

```php
use FlowCatalyst\Facades\FlowCatalyst;
use FlowCatalyst\DTOs\Requests\CreateDispatchPoolRequest;

// Create a dispatch pool for rate limiting (returns the new id)
$poolId = FlowCatalyst::dispatchPools()->create(new CreateDispatchPoolRequest(
    code: 'warehouse-webhooks',
    name: 'Warehouse Webhooks',
    rateLimit: 100,      // Max 100 requests per minute
    concurrency: 10,     // Max 10 concurrent requests
));

// Suspend/activate a pool
FlowCatalyst::dispatchPools()->suspend($poolId);
FlowCatalyst::dispatchPools()->activate($poolId);
```

### Roles & Permissions

To **declare** roles/permissions, prefer the attribute sync in
[Declaring & syncing roles and permissions](#declaring--syncing-roles-and-permissions).
For reads and ad-hoc lookups:

```php
use FlowCatalyst\Facades\FlowCatalyst;

$roles = FlowCatalyst::roles()->list();
$role = FlowCatalyst::roles()->get('myapp:admin');   // includes its permissions
$permissions = FlowCatalyst::permissions()->list();
```

### Applications

```php
use FlowCatalyst\Facades\FlowCatalyst;
use FlowCatalyst\DTOs\Requests\CreateApplicationRequest;

// List applications
$result = FlowCatalyst::applications()->list();

// Get by code
$app = FlowCatalyst::applications()->getByCode('myapp');

// Create an application
$app = FlowCatalyst::applications()->create(new CreateApplicationRequest(
    code: 'myapp',
    name: 'My Application',
    description: 'My awesome application',
));
```

## Portal identities

Portal users (`ptu_…`) are a separate end-user population from platform
users — a client's customers, invited per client, with no platform SSO
reuse. For the login side, set `FLOWCATALYST_OIDC_PORTAL=true` so the OIDC
flow enters through `/portal/authorize`. For managing portal users, the
full admin surface lives on the generated client:

```php
use FlowCatalyst\Facades\FlowCatalyst;
use FlowCatalyst\Generated\Model\PortalUserRequest;

$api = FlowCatalyst::generated();

// Invite (idempotent): returns identityId, created/invited flags, inviteUrl
$result = $api->ensurePortalUser((new PortalUserRequest())
    ->setClientId($clientId)
    ->setEmail('pat@customer.example')
    ->setReturnInviteLink(true));

$users = $api->listPortalUsers(['clientId' => $clientId]);
```

`activatePortalUser` / `deactivatePortalUser` / `deletePortalUser` cover the
rest of the lifecycle. All credential UX (login page, invites, self-service
forgot-password) is platform-hosted — the portal app never implements any
of it. Full walkthrough:
[docs/portal-implementation-guide.md](../../docs/portal-implementation-guide.md).

## Outbox (Event Creation)

The outbox allows your application to create events and dispatch jobs using the outbox pattern. Events are written to a local database table and then forwarded to FlowCatalyst by a poller (`fcdev outbox poll` locally, or the standalone outbox poller in production).

### Setup

Publish and run the migration:

```bash
php artisan vendor:publish --tag=flowcatalyst-migrations
php artisan migrate
```

### Creating Events

```php
use FlowCatalyst\Facades\Outbox;
use FlowCatalyst\Outbox\DTOs\CreateEventDto;

// Create a single event
$eventId = Outbox::createEvent(
    CreateEventDto::create(
        type: 'order.created',
        data: ['orderId' => 'ORD-123', 'total' => 99.99, 'currency' => 'USD'],
    )->withCorrelationId('corr-abc-123')
      ->withSource('order-service')
      ->withMessageGroup('orders')   // per-group FIFO ordering
);

// Batch create events
$eventIds = Outbox::createEvents([
    CreateEventDto::create('order.created', ['orderId' => 'ORD-001']),
    CreateEventDto::create('order.created', ['orderId' => 'ORD-002']),
    CreateEventDto::create('order.created', ['orderId' => 'ORD-003']),
]);
```

### Client-centric linkage

FlowCatalyst is client-centric. Name the application and client your emitted
events / audit logs belong to once, and the platform resolves them at ingest:

```env
FLOWCATALYST_APP_CODE=blog              # application (also the event-type prefix)
FLOWCATALYST_CLIENT_CODE=acme           # client (by code → client_id at ingest)
```

Events emitted through the use-case envelope carry the **client** code (the
application is derived from the event-type prefix); **audit logs** carry both.
An unknown / unset client code simply leaves the link empty — the event still
ingests (an event is a fact; the platform keeps it).

### Branded IDs

Mint platform-style typed ids for your own entities (matching `aud_…`, `prn_…`):

```php
use FlowCatalyst\UseCase\DomainEventHelpers;

DomainEventHelpers::generateId();        // raw 13-char TSID
DomainEventHelpers::generateId('cmt');   // branded → "cmt_6F7JC2A6JFR7N"
```

### Creating Dispatch Jobs

```php
use FlowCatalyst\Facades\Outbox;
use FlowCatalyst\Outbox\DTOs\CreateDispatchJobDto;

// Create a dispatch job (direct webhook without subscription matching)
$jobId = Outbox::createDispatchJob(
    CreateDispatchJobDto::create(
        source: 'order-service',
        code: 'notify-warehouse',
        targetUrl: 'https://warehouse.example.com/webhook',
        payload: ['orderId' => 'ORD-123', 'action' => 'prepare'],
        dispatchPoolId: $warehousePoolId,
    )->withCorrelationId('corr-abc-123')
      ->withHeaders(['X-Priority' => 'high'])
);
```

## Webhook Validation

Validate incoming webhooks from FlowCatalyst using HMAC-SHA256 signatures.

### Using Middleware

```php
// routes/api.php
Route::post('/webhooks/flowcatalyst', [WebhookController::class, 'handle'])
    ->middleware('flowcatalyst.webhook');
```

### Manual Validation

```php
use FlowCatalyst\Webhook\WebhookValidator;
use FlowCatalyst\Exceptions\WebhookValidationException;

public function handleWebhook(Request $request)
{
    try {
        $validator = WebhookValidator::fromConfig();
        $validator->validateRequest($request);
    } catch (WebhookValidationException $e) {
        return response()->json(['error' => 'Invalid signature'], 401);
    }

    // Process the webhook
    $payload = $request->json()->all();

    return response()->json(['received' => true]);
}
```

## Database Requirements

### MySQL 8.0+

MySQL 8.0 or higher is required for native JSON column support.

```bash
composer require doctrine/dbal
php artisan vendor:publish --tag=flowcatalyst-migrations
php artisan migrate
```

### PostgreSQL 12+

PostgreSQL 12+ is fully supported with JSONB columns.

```bash
composer require doctrine/dbal
php artisan vendor:publish --tag=flowcatalyst-migrations
php artisan migrate
```

### MongoDB 4.4+

For MongoDB, install the Laravel MongoDB package:

```bash
composer require mongodb/laravel-mongodb
```

Add to `config/database.php`:

```php
'mongodb' => [
    'driver' => 'mongodb',
    'host' => env('MONGO_HOST', 'localhost'),
    'port' => env('MONGO_PORT', 27017),
    'database' => env('MONGO_DATABASE'),
    'username' => env('MONGO_USERNAME'),
    'password' => env('MONGO_PASSWORD'),
],
```

Configure in `.env`:

```env
FLOWCATALYST_OUTBOX_DRIVER=mongodb
FLOWCATALYST_OUTBOX_CONNECTION=mongodb
```

Create the collection with indexes:

```javascript
db.createCollection('outbox_messages');
db.outbox_messages.createIndex(
  { clientId: 1, status: 1, createdAt: 1 },
  { name: 'idx_outbox_pending' },
);
db.outbox_messages.createIndex({ status: 1, createdAt: 1 }, { name: 'idx_outbox_status' });
```

## Error Handling

The SDK throws specific exceptions for different error types:

```php
use FlowCatalyst\Exceptions\FlowCatalystException;
use FlowCatalyst\Exceptions\AuthenticationException;
use FlowCatalyst\Exceptions\ValidationException;
use FlowCatalyst\Exceptions\OutboxException;

try {
    FlowCatalyst::eventTypes()->create(new CreateEventTypeRequest(...));
} catch (AuthenticationException $e) {
    // Invalid credentials or token expired
} catch (ValidationException $e) {
    // Validation errors
    $errors = $e->getErrors();
} catch (FlowCatalystException $e) {
    // General API error
    $context = $e->getContext();
}
```

## AI Agent Access (MCP Server)

If you're using an AI coding agent (Claude Code, Cursor, Windsurf, etc.), you can give it read-only access to your FlowCatalyst event types, schemas, and subscriptions via the MCP server. This lets the agent explore your event catalog and generate typed code (including PHP DTOs) for you.

### Quick setup (Claude Code)

```bash
claude mcp add flowcatalyst -- npx @flowcatalyst/mcp-server
```

### Quick setup (Cursor / Windsurf / Claude Desktop)

Add to your MCP config file (`.cursor/mcp.json`, Claude Desktop config, etc.):

```json
{
  "mcpServers": {
    "flowcatalyst": {
      "command": "npx",
      "args": ["@flowcatalyst/mcp-server"],
      "env": {
        "FLOWCATALYST_URL": "https://your-instance.flowcatalyst.io",
        "FLOWCATALYST_CLIENT_ID": "svc_abc123",
        "FLOWCATALYST_CLIENT_SECRET": "your_secret"
      }
    }
  }
}
```

You need a service account with the `AI Agent Read-Only` role. The agent can then generate PHP DTOs, TypeScript interfaces, Python dataclasses, or Java records from your event schemas. See the [MCP server README](../mcp-server/README.md) for full details.

## Testing

For testing, you can mock the facades:

```php
use FlowCatalyst\Facades\FlowCatalyst;
use FlowCatalyst\Facades\Outbox;

FlowCatalyst::shouldReceive('eventTypes->list')
    ->andReturn(new EventTypeList(items: [], total: 0));

Outbox::shouldReceive('createEvent')
    ->andReturn('0HZXEQ5Y8JY5Z');
```

## License

Copyright (c) 2026 Belac (FlowCatalyst).

[MPL-2.0](https://www.mozilla.org/en-US/MPL/2.0/) — see [LICENSE](LICENSE).
File-level copyleft: changes to the SDK's own files must be shared; your
application that depends on it is not affected.
