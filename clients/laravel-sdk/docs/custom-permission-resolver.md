# Custom permission resolver

When a request authenticates with a FlowCatalyst access token (a user or a
service-account / `client_credentials` token), the `fc-token` guard validates
the token **offline** (JWKS, no network call), reads the token's `roles` claim,
and then asks a **`PermissionResolver`** to turn those roles into the
principal's effective permission codes. Those codes back
`$auth->hasPermissionTo([...])` in your controllers.

The SDK ships a default resolver. You can replace it with your own — for
example one that reads from your app's local Spatie permission tables.

## The contract

```php
namespace FlowCatalyst\Auth\Contracts;

interface PermissionResolver
{
    /**
     * @param string[]    $roles The principal's FlowCatalyst role names (from the token).
     * @param string|null $token The raw bearer token — only useful for server-backed
     *                           resolvers; a DB/offline resolver ignores it.
     * @return string[] Effective permission codes ("app:context:aggregate:action").
     */
    public function resolve(array $roles, ?string $token = null): array;
}
```

The guard calls `resolve($user->getRoles(), $token)` and stamps the result onto
the principal. Permission checks then match the returned codes with wildcard
support (see below).

## Built-in resolvers + precedence

The service provider binds a default via `bindIf`, with this precedence:

1. **App-bound `PermissionResolver`** — if you bind the interface, yours wins
   (regardless of provider order).
2. **`CataloguePermissionResolver`** — used automatically if you've bound an
   `RbacCatalogue` (offline, resolves roles → permissions from a static
   catalogue you ship in code).
3. **`ApiMePermissionResolver`** (default) — calls `GET /api/me` with the
   bearer token, returns the server-resolved `permissions`, caches by
   `md5(token)` for `oidc.me_cache_ttl_seconds` (default 60s), fails closed
   (`[]`) on any error.

## DB-backed Spatie resolver

Use this when the app already mirrors FlowCatalyst roles/permissions into its
local Spatie tables and you'd rather resolve from the DB than call `/api/me`.

```php
namespace App\FlowCatalyst;

use FlowCatalyst\Auth\Contracts\PermissionResolver;
use Illuminate\Support\Facades\Cache;
use Spatie\Permission\Models\Role;

final class SpatiePermissionResolver implements PermissionResolver
{
    public function __construct(private readonly string $guard = 'web') {}

    public function resolve(array $roles, ?string $token = null): array
    {
        if ($roles === []) {
            return [];
        }
        sort($roles);

        // Cache by ROLE SET (not the token) so every token sharing the same
        // roles shares one DB lookup.
        return Cache::remember(
            'fc.perms.' . md5($this->guard . '|' . implode(',', $roles)),
            60,
            fn (): array => Role::query()
                ->where('guard_name', $this->guard)
                ->whereIn('name', $roles)
                ->with('permissions:id,name')
                ->get()
                ->flatMap(fn (Role $r) => $r->permissions->pluck('name'))
                ->unique()->values()->all(),
        );
    }
}
```

### Wiring (this is all that's needed)

In `App\Providers\AppServiceProvider::register()`:

```php
use App\FlowCatalyst\SpatiePermissionResolver;
use FlowCatalyst\Auth\Contracts\PermissionResolver;

$this->app->bind(PermissionResolver::class, fn () => new SpatiePermissionResolver(guard: 'web'));
```

The SDK registers its default with `bindIf`, so this explicit `bind` always
wins. No config changes, no guard changes.

### Using it (controllers unchanged)

Protect routes with the token guard (Passport-fallback variant) and check
permissions exactly as before:

```php
Route::middleware('auth:fc-token,api')->group(function () {
    // ...
});

// in a controller / action
if (! $auth->hasPermissionTo(['integral:orders:order:view'])) {
    abort(403);
}
```

`auth:fc-token,api` tries the FlowCatalyst token first and falls through to
Passport (`api`) for non-FlowCatalyst callers. The `fc.or-passport` middleware
does the same as a single alias.

## Four things to get right

1. **Role-name match.** The token's `roles` claim must line up with Spatie
   `roles.name`. FlowCatalyst forbids colons in role names and prefixes the app
   code platform-side — confirm whether the claim arrives bare
   (`administrator`) or prefixed (`integral:administrator`), and make your
   Spatie `roles.name` match, or map the names before the `whereIn`.
2. **Permission code format.** Spatie `permissions.name` must store the
   canonical lowercase `app:context:aggregate:action` strings your controllers
   check.
3. **Wildcards are free.** Permission matching uses `RbacCatalogue::matches`, so
   a stored grant of `integral:orders:*:*` satisfies a check for
   `integral:orders:order:view`. Store wildcard rows for coarse grants.
4. **Scope is separate from permissions.** This resolver only fills
   *permissions*. App/client scoping (`hasClientAccess()`, `hasFullAccess()`,
   `getApplications()`) still comes from the token's `clients` / `applications`
   claims — don't try to derive those from Spatie.

## Failure mode

Decide whether a DB error should fail the request (let the exception propagate)
or fail closed (catch → return `[]`, so the caller simply has no permissions and
gets a 403). The default `/api/me` resolver fails closed.
