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
} from "./resources";

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

// UseCase / UnitOfWork — domain-driven write pattern with outbox dispatch.
// Exported as a namespace to avoid clashing with neverthrow's `Result` and the
// HTTP `ValidationError`/`NotFoundError` types. Typical usage:
//
//   import { usecase } from "@flowcatalyst/sdk";
//   class ShipOrderUseCase implements usecase.UseCase<ShipOrderCommand, OrderShipped> { ... }
//   const uow = new usecase.OutboxUnitOfWork({ outboxManager });
//
export * as usecase from "./usecase/index.js";

// Re-export neverthrow utilities for convenience
export { ok, err, Result, ResultAsync } from "neverthrow";
