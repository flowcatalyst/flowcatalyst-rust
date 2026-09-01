# FlowCatalyst Laravel SDK

Official Laravel SDK for the FlowCatalyst Platform - Event-driven architecture made simple.

## Requirements

- PHP 8.1+
- Laravel 10.0+
- MySQL 8.0+ / PostgreSQL 12+ / MongoDB 4.4+ (for postbox)

## Installation

```bash
composer require flowcatalyst/laravel-sdk
```

Publish the configuration file:

```bash
php artisan vendor:publish --tag=flowcatalyst-config
```

## Local development with `fc-dev`

For local work you need a FlowCatalyst control plane to talk to.
`fc-dev` is the official one-binary dev environment — bundled
PostgreSQL, platform API, message router, scheduler, and frontend
in a single process.

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/flowcatalyst/flowcatalyst/main/install.sh | sh

# Windows (PowerShell)
irm https://raw.githubusercontent.com/flowcatalyst/flowcatalyst/main/install.ps1 | iex

fc-dev          # starts API on http://localhost:8080
```

If you use the **Postbox** (outbox pattern), you also need
`fc-dev outbox` running as a sidecar — it polls your app's
`outbox_messages` table and forwards events to the platform.
This sits alongside the `php artisan flowcatalyst:postbox:dispatch`
queue worker (or replaces it for setups where you'd rather not run
Laravel's queue runner for outbox forwarding).

```bash
# In your Laravel project directory:

# Once: write FC_OUTBOX_DB_URL / FC_OUTBOX_API_URL / FC_OUTBOX_TOKEN
# into ./.env (0600 perms; no secrets on argv or shell history).
# fc-dev outbox init appends to your existing .env — your other
# FLOWCATALYST_* keys are not touched.
fc-dev outbox init

# Daily: reads .env, auto-creates the `outbox_messages` table on
# first run if your Postbox migration hasn't been run yet, then polls.
fc-dev outbox poll
```

Complete reference: [fc-dev CLI docs](https://github.com/flowcatalyst/flowcatalyst-rust/blob/main/docs/developers/fc-dev.md).

## Configuration

Add the following to your `.env` file:

```env
# FlowCatalyst API
FLOWCATALYST_BASE_URL=https://your-instance.flowcatalyst.io
FLOWCATALYST_CLIENT_ID=your_client_id
FLOWCATALYST_CLIENT_SECRET=your_client_secret

# Postbox (for event creation)
FLOWCATALYST_TENANT_ID=your_tenant_id
FLOWCATALYST_POSTBOX_DRIVER=database

# Webhook validation (optional)
FLOWCATALYST_SIGNING_SECRET=your_signing_secret
```

## Control Plane API

The SDK provides access to FlowCatalyst control plane APIs using OIDC client credentials authentication.

### Event Types

```php
use FlowCatalyst\Facades\FlowCatalyst;

// List event types
$result = FlowCatalyst::eventTypes()->list();
foreach ($result['items'] as $eventType) {
    echo $eventType->code . ': ' . $eventType->name;
}

// Create an event type
$eventType = FlowCatalyst::eventTypes()->create([
    'code' => 'order:fulfillment:order:created',
    'name' => 'Order Created',
    'description' => 'Fired when a new order is placed',
]);

// Add a schema version
FlowCatalyst::eventTypes()->addSchema($eventType->id, [
    'version' => '1.0',
    'mimeType' => 'application/json',
    'schema' => json_encode(['type' => 'object', 'properties' => [...]]),
    'schemaType' => 'JSON_SCHEMA',
]);

// Finalise the schema
FlowCatalyst::eventTypes()->finaliseSchema($eventType->id, '1.0');
```

### Subscriptions

```php
use FlowCatalyst\Facades\FlowCatalyst;
use FlowCatalyst\DTOs\EventTypeBinding;
use FlowCatalyst\Enums\DispatchMode;

// Create a subscription
$subscription = FlowCatalyst::subscriptions()->create([
    'code' => 'notify-warehouse',
    'name' => 'Notify Warehouse',
    'eventTypes' => [
        ['eventTypeCode' => 'order:fulfillment:order:created'],
    ],
    'target' => 'https://warehouse.example.com/webhook',
    'queue' => 'default',
    'dispatchPoolId' => $poolId,
    'mode' => DispatchMode::IMMEDIATE,
    'timeoutSeconds' => 30,
    'maxRetries' => 5,
]);

// Pause/resume a subscription
FlowCatalyst::subscriptions()->pause($subscription->id);
FlowCatalyst::subscriptions()->resume($subscription->id);
```

### Dispatch Pools

```php
use FlowCatalyst\Facades\FlowCatalyst;

// Create a dispatch pool for rate limiting
$pool = FlowCatalyst::dispatchPools()->create([
    'code' => 'warehouse-webhooks',
    'name' => 'Warehouse Webhooks',
    'rateLimit' => 100,      // Max 100 requests per minute
    'concurrency' => 10,     // Max 10 concurrent requests
]);

// Suspend/activate a pool
FlowCatalyst::dispatchPools()->suspend($pool->id);
FlowCatalyst::dispatchPools()->activate($pool->id);
```

### Roles & Permissions

```php
use FlowCatalyst\Facades\FlowCatalyst;

// List roles
$result = FlowCatalyst::roles()->list();

// Sync roles for your application (SDK-managed roles)
$result = FlowCatalyst::roles()->sync('myapp', [
    [
        'name' => 'admin',
        'displayName' => 'Administrator',
        'description' => 'Full access to all features',
        'permissions' => ['myapp:users:read', 'myapp:users:write', 'myapp:settings:manage'],
    ],
    [
        'name' => 'viewer',
        'displayName' => 'Viewer',
        'permissions' => ['myapp:users:read'],
    ],
], removeUnlisted: true);

// List permissions
$permissions = FlowCatalyst::permissions()->list();
```

### Applications

```php
use FlowCatalyst\Facades\FlowCatalyst;

// List applications
$result = FlowCatalyst::applications()->list();

// Get by code
$app = FlowCatalyst::applications()->getByCode('myapp');

// Create an application
$app = FlowCatalyst::applications()->create([
    'code' => 'myapp',
    'name' => 'My Application',
    'description' => 'My awesome application',
]);
```

## Postbox (Event Creation)

The postbox allows your application to create events and dispatch jobs using the outbox pattern. Events are written to a local database table and then processed by FlowCatalyst.

### Setup

Publish and run the migration:

```bash
php artisan vendor:publish --tag=flowcatalyst-migrations
php artisan migrate
```

### Creating Events

```php
use FlowCatalyst\Facades\Postbox;
use FlowCatalyst\Postbox\DTOs\CreateEventDto;

// Create a single event
$eventId = Postbox::createEvent(
    CreateEventDto::create(
        type: 'order.created',
        data: ['orderId' => 'ORD-123', 'total' => 99.99, 'currency' => 'USD'],
        partitionId: 'orders'
    )->withCorrelationId('corr-abc-123')
      ->withSource('order-service')
);

// Batch create events
$eventIds = Postbox::createEvents([
    CreateEventDto::create('order.created', ['orderId' => 'ORD-001'], 'orders'),
    CreateEventDto::create('order.created', ['orderId' => 'ORD-002'], 'orders'),
    CreateEventDto::create('order.created', ['orderId' => 'ORD-003'], 'orders'),
]);
```

### Creating Dispatch Jobs

```php
use FlowCatalyst\Facades\Postbox;
use FlowCatalyst\Postbox\DTOs\CreateDispatchJobDto;

// Create a dispatch job (direct webhook without subscription matching)
$jobId = Postbox::createDispatchJob(
    CreateDispatchJobDto::create(
        source: 'order-service',
        code: 'notify-warehouse',
        targetUrl: 'https://warehouse.example.com/webhook',
        payload: ['orderId' => 'ORD-123', 'action' => 'prepare'],
        dispatchPoolId: $warehousePoolId,
        partitionId: 'warehouse-notifications'
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
FLOWCATALYST_POSTBOX_DRIVER=mongodb
FLOWCATALYST_POSTBOX_CONNECTION=mongodb
```

Create the collection with indexes:

```javascript
db.createCollection('postbox_messages');
db.postbox_messages.createIndex(
  { tenant_id: 1, partition_id: 1, status: 1, created_at: 1 },
  { name: 'idx_postbox_pending' },
);
db.postbox_messages.createIndex({ status: 1, created_at: 1 }, { name: 'idx_postbox_status' });
```

## Error Handling

The SDK throws specific exceptions for different error types:

```php
use FlowCatalyst\Exceptions\FlowCatalystException;
use FlowCatalyst\Exceptions\AuthenticationException;
use FlowCatalyst\Exceptions\ValidationException;
use FlowCatalyst\Exceptions\PostboxException;

try {
    FlowCatalyst::eventTypes()->create([...]);
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
use FlowCatalyst\Facades\Postbox;

FlowCatalyst::shouldReceive('eventTypes->list')
    ->andReturn(['items' => [], 'total' => 0]);

Postbox::shouldReceive('createEvent')
    ->andReturn('0HZXEQ5Y8JY5Z');
```

## License

Copyright (c) 2026 Belac (FlowCatalyst).

[MPL-2.0](https://www.mozilla.org/en-US/MPL/2.0/) — see [LICENSE](LICENSE).
File-level copyleft: changes to the SDK's own files must be shared; your
application that depends on it is not affected.
