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
export { FlowCatalystClient, type FlowCatalystConfig, type ClientCredentialsConfig, type UserTokenConfig, } from "./client";
export { OidcTokenManager, type TokenManagerConfig } from "./auth";
export type { SdkError, AuthenticationError, HttpError, ValidationError, NotFoundError, ForbiddenError, ConflictError, RateLimitError, } from "./errors";
export { authError, httpError, validationError, notFoundError, forbiddenError, conflictError, rateLimitError, mapHttpStatusToError, } from "./errors";
export { EventTypesResource, SubscriptionsResource, DispatchPoolsResource, ConnectionsResource, RolesResource, PermissionsResource, ApplicationsResource, ClientsResource, PrincipalsResource, } from "./resources";
export type * from "./generated/types.gen";
export { OutboxManager, OutboxStatus } from "./outbox/index.js";
export type { OutboxDriver, OutboxMessage, OutboxStatusCode, MessageType, } from "./outbox/index.js";
export { CreateEventDto } from "./outbox/index.js";
export { CreateDispatchJobDto } from "./outbox/index.js";
export { CreateAuditLogDto } from "./outbox/index.js";
export { generateTsid, isValidTsid } from "./outbox/index.js";
export * as usecase from "./usecase/index.js";
export { ok, err, Result, ResultAsync } from "neverthrow";
//# sourceMappingURL=index.d.ts.map