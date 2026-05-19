/**
 * FlowCatalyst SDK for TypeScript/JavaScript
 *
 * A typed client library for the FlowCatalyst platform with neverthrow
 * for explicit error handling.
 *
 * @example
 * ```typescript
 * import { FlowCatalystClient } from '@flowcatalyst/sdk';
 *
 * const client = new FlowCatalystClient({
 *   baseUrl: 'https://your-instance.flowcatalyst.io',
 *   clientId: 'your_client_id',
 *   clientSecret: 'your_client_secret',
 * });
 *
 * // All methods return ResultAsync for typed error handling
 * const result = await client.eventTypes().list();
 *
 * // Pattern matching on results
 * result.match(
 *   (eventTypes) => console.log('Success:', eventTypes),
 *   (error) => {
 *     switch (error.type) {
 *       case 'validation':
 *         console.error('Validation error:', error.errors);
 *         break;
 *       case 'not_found':
 *         console.error('Not found:', error.message);
 *         break;
 *       default:
 *         console.error('Error:', error.message);
 *     }
 *   }
 * );
 *
 * // Or use isOk/isErr guards
 * if (result.isOk()) {
 *   console.log('Event types:', result.value);
 * }
 * ```
 *
 * @packageDocumentation
 */

// Main client
export {
	FlowCatalystClient,
	type FlowCatalystConfig,
	type ClientCredentialsConfig,
	type UserTokenConfig,
} from "./client";

// Authentication
export { OidcTokenManager, type TokenManagerConfig } from "./auth";

// Error types
export type {
	SdkError,
	AuthenticationError,
	HttpError,
	ValidationError,
	NotFoundError,
	ForbiddenError,
	ConflictError,
	RateLimitError,
} from "./errors";
export {
	authError,
	httpError,
	validationError,
	notFoundError,
	forbiddenError,
	conflictError,
	rateLimitError,
	mapHttpStatusToError,
} from "./errors";

// Resource classes
export {
	EventTypesResource,
	SubscriptionsResource,
	DispatchPoolsResource,
	ConnectionsResource,
	RolesResource,
	PermissionsResource,
	ApplicationsResource,
	ClientsResource,
	PrincipalsResource,
	ScheduledJobsResource,
	AuditLogsResource,
} from "./resources";

// Scheduled-job runner (handler registration + lock + completion callback).
export {
	ScheduledJobRunner,
	type ScheduledJobEnvelope,
	type Handler as ScheduledJobHandler,
	type HandlerContext as ScheduledJobHandlerContext,
	type RunnerOptions as ScheduledJobRunnerOptions,
	type RunResult as ScheduledJobRunResult,
} from "./runner/scheduled-job-runner";
export {
	type LockProvider,
	type LockHandle,
	NoOpLockProvider,
	InMemoryLockProvider,
} from "./runner/lock-provider";
export {
	PgLockProvider,
	type PgLockProviderOptions,
} from "./runner/pg-lock-provider";
export {
	RedisLockProvider,
	type RedisLockCommandable,
	type RedisLockProviderOptions,
} from "./runner/redis-lock-provider";
export {
	CREATE_LOCK_TABLE_SQL,
	initLockSchema,
	initLockSchemaWithTable,
} from "./runner/lock-schema";

// Cache — pluggable key-value cache with required TTL. Three backends:
// MemoryCacheStore (default for tests/dev), PgCacheStore (node-postgres-
// compatible), RedisCacheStore (ioredis-compatible).
export {
	CacheError,
	type CacheStore,
	MemoryCacheStore,
	PgCacheStore,
	RedisCacheStore,
	type RedisCommandable,
	type RedisCacheStoreOptions,
	CREATE_CACHE_TABLE_SQL,
	initCacheSchema,
	initCacheSchemaWithTable,
} from "./cache/index.js";

// Scheduled-job DTOs (re-exported here so consumers don't need to drill in).
export type {
	ScheduledJob,
	ScheduledJobInstance,
	ScheduledJobInstanceLog,
	ScheduledJobStatus,
	TriggerKind,
	InstanceStatus,
	CompletionStatus,
	LogLevel,
	CreateScheduledJobRequest,
	UpdateScheduledJobRequest,
	ListJobsFilters,
	ListInstancesFilters,
	FireRequest,
	InstanceLogRequest,
	InstanceCompleteRequest,
	PaginatedJobs,
	PaginatedInstances,
} from "./resources/scheduled-jobs";

// Re-export generated types for convenience
export type * from "./generated/types.gen";

// Outbox - transactional outbox pattern
export { OutboxManager, OutboxStatus } from "./outbox/index.js";
export type {
	OutboxDriver,
	OutboxMessage,
	OutboxStatusCode,
	MessageType,
} from "./outbox/index.js";
export { CreateEventDto } from "./outbox/index.js";
export { CreateDispatchJobDto } from "./outbox/index.js";
export { CreateAuditLogDto } from "./outbox/index.js";
export { generateTsid, isValidTsid } from "./outbox/index.js";
export { PgOutboxDriver } from "./outbox/index.js";
export type {
	PgQueryable,
	PgPoolLike,
	PgPoolClientLike,
} from "./outbox/index.js";

// UseCase / UnitOfWork — domain-driven write pattern with outbox dispatch.
// Exported as a namespace to avoid clashing with neverthrow's `Result` and the
// HTTP `ValidationError`/`NotFoundError` types. Typical usage:
//
//   import { usecase } from "@flowcatalyst/sdk";
//   class ShipOrderUseCase implements usecase.UseCase<ShipOrderCommand, OrderShipped> { ... }
//   const uow = new usecase.OutboxUnitOfWork({ outboxManager });
//
export * as usecase from "./usecase/index.js";

// Sync — declarative definitions (roles, event types, subscriptions,
// dispatch pools, principals) pushed to the platform per application.
// See `docs/syncing-definitions.md` for structure conventions.
//
//   import { sync, FlowCatalystClient } from "@flowcatalyst/sdk";
//   const set = sync.defineApplication("orders").withRoles([...]).build();
//   await client.definitions().sync(set);
//
export * as sync from "./sync/index.js";

// Re-export neverthrow utilities for convenience
export { ok, err, Result, ResultAsync } from "neverthrow";
