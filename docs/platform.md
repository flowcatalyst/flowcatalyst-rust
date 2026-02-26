# Platform Service

The Platform Service provides REST APIs for managing the FlowCatalyst platform, including events, subscriptions, clients, users, and administration.

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                          Platform Server                                 │
│                                                                          │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                         Axum Router                              │   │
│  │  ┌───────────┐  ┌───────────┐  ┌───────────┐  ┌───────────┐    │   │
│  │  │  BFF APIs │  │ Admin APIs│  │ Auth APIs │  │Monitoring │    │   │
│  │  └───────────┘  └───────────┘  └───────────┘  └───────────┘    │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                                    │                                    │
│                                    ▼                                    │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                       Auth Middleware                            │   │
│  │              JWT Validation │ RBAC │ Multi-tenant               │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                                    │                                    │
│                                    ▼                                    │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                        Service Layer                             │   │
│  │  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐            │   │
│  │  │  Auth   │  │ Dispatch│  │  Audit  │  │  OIDC   │            │   │
│  │  │ Service │  │ Service │  │ Service │  │ Service │            │   │
│  │  └─────────┘  └─────────┘  └─────────┘  └─────────┘            │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                                    │                                    │
│                                    ▼                                    │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                      Repository Layer                            │   │
│  │  MongoDB Collections: events, subscriptions, clients, etc.       │   │
│  └─────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
                            ┌─────────────┐
                            │   MongoDB   │
                            └─────────────┘
```

## Domain Model

### Core Entities

| Entity | Collection | Description |
|--------|------------|-------------|
| `Event` | `events` | Published events with payload and metadata |
| `EventType` | `event_types` | Event type definitions with versioning |
| `Subscription` | `subscriptions` | Webhook subscriptions with filters |
| `DispatchJob` | `dispatch_jobs` | Delivery jobs with lifecycle tracking |
| `DispatchPool` | `dispatch_pools` | Processing pool configurations |

### Identity & Access

| Entity | Collection | Description |
|--------|------------|-------------|
| `Client` | `clients` | Tenant/customer organizations |
| `Principal` | `principals` | Users and service accounts |
| `Role` | `roles` | Permission roles |
| `Application` | `applications` | Registered applications |
| `ServiceAccount` | `service_accounts` | Machine-to-machine accounts |
| `OAuthClient` | `oauth_clients` | OAuth 2.0 client configurations |

### Configuration

| Entity | Collection | Description |
|--------|------------|-------------|
| `AuthConfig` | `auth_configs` | Client authentication settings |
| `AnchorDomain` | `anchor_domains` | Platform admin email domains |
| `IdpRoleMapping` | `idp_role_mappings` | External IdP to internal role mappings |
| `AuditLog` | `audit_logs` | Audit trail of operations |

## API Structure

### BFF APIs (Backend-for-Frontend)

Optimized for UI consumption with filtering, pagination, and aggregation.

| Endpoint | Description |
|----------|-------------|
| `GET /api/bff/events` | List events with filters |
| `GET /api/bff/events/:id` | Event detail with dispatch jobs |
| `GET /api/bff/event-types` | List event types |
| `GET /api/bff/dispatch-jobs` | List dispatch jobs |
| `GET /api/bff/dispatch-jobs/:id` | Dispatch job detail |
| `GET /api/bff/filter-options` | Filter dropdown options |

### Admin APIs

CRUD operations for platform management.

| Resource | Endpoints |
|----------|-----------|
| `/api/admin/clients` | Client management |
| `/api/admin/principals` | User/service account management |
| `/api/admin/roles` | Role management |
| `/api/admin/subscriptions` | Subscription management |
| `/api/admin/applications` | Application management |
| `/api/admin/dispatch-pools` | Dispatch pool configuration |
| `/api/admin/oauth-clients` | OAuth client management |
| `/api/admin/anchor-domains` | Anchor domain configuration |
| `/api/admin/client-auth-configs` | Client auth settings |
| `/api/admin/idp-role-mappings` | IdP role mappings |
| `/api/admin/audit-logs` | Audit log access |

### Auth APIs

| Endpoint | Description |
|----------|-------------|
| `POST /api/auth/login` | Username/password login |
| `POST /api/auth/token` | Token refresh |
| `GET /api/auth/oidc/:provider/login` | OIDC login initiation |
| `GET /api/auth/oidc/:provider/callback` | OIDC callback |
| `POST /api/auth/password-reset/request` | Password reset request |
| `POST /api/auth/password-reset/confirm` | Password reset confirmation |

### Monitoring APIs

| Endpoint | Description |
|----------|-------------|
| `GET /api/monitoring/health` | Health status |
| `GET /api/monitoring/leader` | Leader election status |
| `GET /api/monitoring/circuit-breakers` | Circuit breaker states |
| `GET /api/monitoring/in-flight` | In-flight request count |

## Services

### AuthService (`fc-platform/src/service/auth.rs`)

JWT-based authentication:
- RSA key pair management (auto-generate or load from file/env)
- Token generation with configurable claims
- Token validation and refresh
- Password hashing with Argon2

### AuthorizationService (`fc-platform/src/service/authorization.rs`)

Role-based access control:
- Permission checking against roles
- Multi-tenant scope validation
- Resource-level authorization

### AuditService (`fc-platform/src/service/audit.rs`)

Audit logging:
- Operation tracking (create, update, delete)
- Actor identification
- Before/after state capture

### DispatchService (`fc-platform/src/service/dispatch.rs`)

Dispatch job management:
- Job creation from events
- Status updates and lifecycle
- Retry coordination

### OIDCService (`fc-platform/src/service/oidc.rs`)

OIDC/OAuth integration:
- Provider discovery
- Authorization flow handling
- Token exchange
- User info mapping

### ProjectionService (`fc-platform/src/service/projection.rs`)

Read model building:
- Denormalized views for queries
- Event type statistics
- Subscription matching optimization

## Multi-Tenancy

### User Scopes

| Scope | Description | Access |
|-------|-------------|--------|
| `ANCHOR` | Platform administrators | All clients |
| `PARTNER` | Partner organizations | Assigned clients |
| `CLIENT` | Single-tenant users | One client only |

### Scope Resolution

1. **Email domain matching**: Anchor domains → `ANCHOR` scope
2. **Client binding**: Client-bound domains → `CLIENT` scope
3. **Explicit assignment**: Partner principals → `PARTNER` scope

### Token Claims

JWT tokens include a `clients` claim:
- `["*"]` for ANCHOR users (access to all)
- `["CLIENT_ID_1", "CLIENT_ID_2"]` for specific access

## Binary

### fc-platform-server

```bash
cargo build -p fc-platform-server --release
./target/release/fc-platform-server
```

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `FC_API_PORT` | `8080` | HTTP API port |
| `FC_METRICS_PORT` | `9090` | Metrics/health port |
| `FC_MONGO_URL` | `mongodb://localhost:27017` | MongoDB connection |
| `FC_MONGO_DB` | `flowcatalyst` | MongoDB database |
| `FC_JWT_PRIVATE_KEY_PATH` | - | RSA private key file |
| `FC_JWT_PUBLIC_KEY_PATH` | - | RSA public key file |
| `FLOWCATALYST_JWT_PRIVATE_KEY` | - | RSA private key (env) |
| `FLOWCATALYST_JWT_PUBLIC_KEY` | - | RSA public key (env) |
| `FC_JWT_ISSUER` | `flowcatalyst` | JWT issuer claim |
| `RUST_LOG` | `info` | Log level |

### JWT Key Configuration

Keys can be provided in three ways (in order of precedence):

1. **File paths**: Set `FC_JWT_PRIVATE_KEY_PATH` and `FC_JWT_PUBLIC_KEY_PATH`
2. **Environment variables**: Set `FLOWCATALYST_JWT_PRIVATE_KEY` and `FLOWCATALYST_JWT_PUBLIC_KEY`
3. **Auto-generation**: If neither is set, keys are generated and persisted to `.jwt-keys/`

Generate production keys:
```bash
openssl genrsa -out jwt-private.pem 2048
openssl rsa -in jwt-private.pem -pubout -out jwt-public.pem
```

## ID Format (TSID)

All entity IDs use TSIDs (Time-Sorted IDs) as Crockford Base32 strings:
- 13 characters (e.g., `0HZXEQ5Y8JY5Z`)
- Lexicographically sortable
- URL-safe and case-insensitive
- Safe from JavaScript number precision issues

```rust
use fc_platform::tsid::TsidGenerator;

let id = TsidGenerator::generate();  // "0HZXEQ5Y8JY5Z"
```

## Repository Layer

All repositories implement MongoDB persistence with:
- Index management for query optimization
- TSID string ID handling
- Pagination support
- Multi-tenant filtering

Example repository:
```rust
pub struct EventRepository {
    collection: Collection<Event>,
}

impl EventRepository {
    pub async fn find_by_client(&self, client_id: &str, page: u32, size: u32) -> Result<Vec<Event>>;
    pub async fn find_by_id(&self, id: &str) -> Result<Option<Event>>;
    pub async fn insert(&self, event: Event) -> Result<String>;
}
```

## Middleware

### AuthLayer (`fc-platform/src/api/middleware.rs`)

Request authentication middleware:
1. Extract `Authorization: Bearer <token>` header
2. Validate JWT signature and expiration
3. Extract claims (user ID, client scope, roles)
4. Attach `AuthContext` to request extensions

### Error Handling

Standardized error responses:
```json
{
  "error": "NOT_FOUND",
  "message": "Event not found",
  "details": null
}
```

## Event Lifecycle

```
┌──────────┐     ┌──────────┐     ┌──────────┐     ┌──────────┐
│ CREATED  │────▶│ MATCHED  │────▶│DISPATCHED│────▶│DELIVERED │
└──────────┘     └──────────┘     └──────────┘     └──────────┘
                                        │
                                        ▼
                                 ┌──────────┐
                                 │  FAILED  │
                                 └──────────┘
```

## Crate Structure

```
fc-platform/
├── src/
│   ├── domain/           # Entity models
│   │   ├── event.rs
│   │   ├── subscription.rs
│   │   ├── client.rs
│   │   └── ...
│   ├── repository/       # MongoDB repositories
│   │   ├── event.rs
│   │   ├── subscription.rs
│   │   └── ...
│   ├── service/          # Business logic
│   │   ├── auth.rs
│   │   ├── audit.rs
│   │   └── ...
│   ├── api/              # HTTP handlers
│   │   ├── events_router.rs
│   │   ├── subscriptions_router.rs
│   │   ├── middleware.rs
│   │   └── ...
│   ├── usecase/          # DDD infrastructure
│   │   ├── context.rs
│   │   ├── error.rs
│   │   └── ...
│   ├── idp/              # Identity providers
│   │   ├── entra.rs
│   │   └── keycloak.rs
│   └── tsid/             # ID generation
└── tests/                # Integration tests
```

## Testing

```bash
# Unit tests
cargo test -p fc-platform

# API integration tests
cargo test -p fc-platform --test api_tests
```

## Dependencies

- `fc-common`: Shared types
- `fc-queue`: Queue publisher (for dispatch)
