/**
 * FlowCatalyst SDK Client
 *
 * Main client for interacting with the FlowCatalyst platform.
 * Uses neverthrow for typed error handling.
 */
import { ResultAsync } from "neverthrow";
import type { Client } from "./generated/client";
import { OidcTokenManager, type TokenManagerConfig } from "./auth";
import type { SdkError } from "./errors";
import { EventTypesResource } from "./resources/event-types";
import { SubscriptionsResource } from "./resources/subscriptions";
import { DispatchPoolsResource } from "./resources/dispatch-pools";
import { RolesResource } from "./resources/roles";
import { PermissionsResource } from "./resources/permissions";
import { ApplicationsResource } from "./resources/applications";
import { ClientsResource } from "./resources/clients";
import { PrincipalsResource } from "./resources/principals";
import { MeResource } from "./resources/me";
import { ConnectionsResource } from "./resources/connections";
import { DefinitionSynchronizer } from "./sync/definition-synchronizer";
/**
 * Configuration for client credentials authentication.
 */
export interface ClientCredentialsConfig extends TokenManagerConfig {
    /** Request timeout in milliseconds (default: 30000) */
    timeout?: number;
    /** Number of retry attempts for transient errors (default: 3) */
    retryAttempts?: number;
    /** Base delay between retries in ms (default: 100) */
    retryDelay?: number;
}
/**
 * Configuration for user token authentication.
 * Use this when you already have a user access token (e.g., from login).
 */
export interface UserTokenConfig {
    /** Base URL of the FlowCatalyst API */
    baseUrl: string;
    /**
     * Access token or a function that returns the current access token.
     * Use a function if the token may change (e.g., refreshed by your app).
     */
    accessToken: string | (() => string) | (() => Promise<string>);
    /** Request timeout in milliseconds (default: 30000) */
    timeout?: number;
    /** Number of retry attempts for transient errors (default: 3) */
    retryAttempts?: number;
    /** Base delay between retries in ms (default: 100) */
    retryDelay?: number;
}
export type FlowCatalystConfig = ClientCredentialsConfig | UserTokenConfig;
/**
 * Main FlowCatalyst SDK client.
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
 * // Using neverthrow Result pattern
 * const result = await client.eventTypes().list();
 * result.match(
 *   (eventTypes) => console.log('Event types:', eventTypes),
 *   (error) => console.error('Error:', error.type, error.message)
 * );
 * ```
 */
export declare class FlowCatalystClient {
    private readonly httpClient;
    private readonly tokenManager;
    private readonly tokenProvider;
    private readonly config;
    private _eventTypes?;
    private _subscriptions?;
    private _dispatchPools?;
    private _roles?;
    private _permissions?;
    private _applications?;
    private _clients?;
    private _principals?;
    private _me?;
    private _connections?;
    private _definitions?;
    constructor(config: FlowCatalystConfig);
    /** Event Types resource */
    eventTypes(): EventTypesResource;
    /** Subscriptions resource */
    subscriptions(): SubscriptionsResource;
    /** Dispatch Pools resource */
    dispatchPools(): DispatchPoolsResource;
    /** Roles resource */
    roles(): RolesResource;
    /** Permissions resource */
    permissions(): PermissionsResource;
    /** Applications resource */
    applications(): ApplicationsResource;
    /** Clients resource */
    clients(): ClientsResource;
    /** Principals resource */
    principals(): PrincipalsResource;
    /** Me resource (user-scoped access to clients and applications) */
    me(): MeResource;
    /** Connections resource */
    connections(): ConnectionsResource;
    /**
     * Definition synchronizer — bulk-sync roles, event types, subscriptions,
     * dispatch pools, and principals per application.
     *
     * See `docs/syncing-definitions.md` for structure and conventions.
     */
    definitions(): DefinitionSynchronizer;
    /**
     * Get the underlying HTTP client (for advanced usage).
     */
    getHttpClient(): Client;
    /**
     * Get the token manager (for advanced usage).
     * Returns null if using user token mode.
     */
    getTokenManager(): OidcTokenManager | null;
    /**
     * Execute an authenticated request with retry logic.
     *
     * Wraps the generated SDK functions with:
     * - Automatic token injection
     * - Retry with exponential backoff
     * - Typed error handling via neverthrow
     */
    request<T>(fn: (client: Client, headers: Record<string, string>) => Promise<{
        data?: unknown;
        error?: unknown;
        response: Response;
    }>): ResultAsync<T, SdkError>;
    private executeWithRetry;
    private executeWithTimeout;
    private isRetryableStatus;
    private delay;
}
//# sourceMappingURL=client.d.ts.map