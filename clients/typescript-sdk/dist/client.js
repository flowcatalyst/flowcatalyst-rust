/**
 * FlowCatalyst SDK Client
 *
 * Main client for interacting with the FlowCatalyst platform.
 * Uses neverthrow for typed error handling.
 */
import { ResultAsync, ok, err, errAsync } from "neverthrow";
import { createClient, createConfig } from "./generated/client";
import { OidcTokenManager } from "./auth";
import { mapHttpStatusToError, httpError, authError } from "./errors";
// Re-export resource classes
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
function isUserTokenConfig(config) {
    return "accessToken" in config;
}
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
export class FlowCatalystClient {
    constructor(config) {
        this.config = {
            timeout: config.timeout ?? 30000,
            retryAttempts: config.retryAttempts ?? 3,
            retryDelay: config.retryDelay ?? 100,
            baseUrl: config.baseUrl.replace(/\/$/, ""),
        };
        if (isUserTokenConfig(config)) {
            // User token mode - use provided token or token provider
            this.tokenManager = null;
            const tokenOrFn = config.accessToken;
            if (typeof tokenOrFn === "string") {
                // Static token
                this.tokenProvider = () => Promise.resolve(tokenOrFn);
            }
            else {
                // Function that returns token (sync or async)
                this.tokenProvider = async () => tokenOrFn();
            }
        }
        else {
            // Client credentials mode - use token manager
            this.tokenManager = new OidcTokenManager({
                baseUrl: this.config.baseUrl,
                clientId: config.clientId,
                clientSecret: config.clientSecret,
                tokenUrl: config.tokenUrl,
            });
            this.tokenProvider = null;
        }
        this.httpClient = createClient(createConfig({
            baseUrl: this.config.baseUrl,
        }));
    }
    // ============ Resource Accessors ============
    /** Event Types resource */
    eventTypes() {
        return (this._eventTypes ?? (this._eventTypes = new EventTypesResource(this)));
    }
    /** Subscriptions resource */
    subscriptions() {
        return (this._subscriptions ?? (this._subscriptions = new SubscriptionsResource(this)));
    }
    /** Dispatch Pools resource */
    dispatchPools() {
        return (this._dispatchPools ?? (this._dispatchPools = new DispatchPoolsResource(this)));
    }
    /** Roles resource */
    roles() {
        return (this._roles ?? (this._roles = new RolesResource(this)));
    }
    /** Permissions resource */
    permissions() {
        return (this._permissions ?? (this._permissions = new PermissionsResource(this)));
    }
    /** Applications resource */
    applications() {
        return (this._applications ?? (this._applications = new ApplicationsResource(this)));
    }
    /** Clients resource */
    clients() {
        return (this._clients ?? (this._clients = new ClientsResource(this)));
    }
    /** Principals resource */
    principals() {
        return (this._principals ?? (this._principals = new PrincipalsResource(this)));
    }
    /** Me resource (user-scoped access to clients and applications) */
    me() {
        return (this._me ?? (this._me = new MeResource(this)));
    }
    /** Connections resource */
    connections() {
        return (this._connections ?? (this._connections = new ConnectionsResource(this)));
    }
    /**
     * Definition synchronizer — bulk-sync roles, event types, subscriptions,
     * dispatch pools, and principals per application.
     *
     * See `docs/syncing-definitions.md` for structure and conventions.
     */
    definitions() {
        return (this._definitions ?? (this._definitions = new DefinitionSynchronizer(this)));
    }
    // ============ Internal Methods ============
    /**
     * Get the underlying HTTP client (for advanced usage).
     */
    getHttpClient() {
        return this.httpClient;
    }
    /**
     * Get the token manager (for advanced usage).
     * Returns null if using user token mode.
     */
    getTokenManager() {
        return this.tokenManager;
    }
    /**
     * Execute an authenticated request with retry logic.
     *
     * Wraps the generated SDK functions with:
     * - Automatic token injection
     * - Retry with exponential backoff
     * - Typed error handling via neverthrow
     */
    request(fn) {
        if (this.tokenProvider) {
            // User token mode
            return ResultAsync.fromPromise(this.tokenProvider(), (e) => authError.tokenExpired(e instanceof Error ? e.message : "Failed to get access token")).andThen((token) => this.executeWithRetry(fn, token, 0, false));
        }
        else if (this.tokenManager) {
            // Client credentials mode
            return this.tokenManager
                .getAccessToken()
                .mapErr((e) => e)
                .andThen((token) => this.executeWithRetry(fn, token, 0, true));
        }
        else {
            return errAsync(authError.tokenExpired("No authentication configured"));
        }
    }
    executeWithRetry(fn, token, attempt, canRefreshToken) {
        const headers = {
            Authorization: `Bearer ${token}`,
        };
        return ResultAsync.fromPromise(this.executeWithTimeout(fn, headers), (e) => {
            if (e instanceof Error && e.name === "AbortError") {
                return httpError.timeout(this.config.timeout);
            }
            return httpError.network(e instanceof Error ? e.message : "Network error", e instanceof Error ? e : undefined);
        }).andThen((result) => {
            // Check if we got an error response
            if (result.error !== undefined || !result.response.ok) {
                const status = result.response.status;
                // Handle 401 with token refresh (only for client credentials mode)
                if (status === 401 &&
                    attempt === 0 &&
                    canRefreshToken &&
                    this.tokenManager) {
                    return this.tokenManager
                        .refreshToken()
                        .mapErr((e) => e)
                        .andThen((newToken) => this.executeWithRetry(fn, newToken, attempt + 1, canRefreshToken));
                }
                // Retry transient errors
                if (this.isRetryableStatus(status) &&
                    attempt < this.config.retryAttempts) {
                    return ResultAsync.fromSafePromise(this.delay(this.config.retryDelay * Math.pow(2, attempt))).andThen(() => this.executeWithRetry(fn, token, attempt + 1, canRefreshToken));
                }
                const sdkError = mapHttpStatusToError(status, result.error);
                return err(sdkError);
            }
            return ok(result.data);
        });
    }
    async executeWithTimeout(fn, headers) {
        const controller = new AbortController();
        const timeoutId = setTimeout(() => controller.abort(), this.config.timeout);
        try {
            // Note: The generated client doesn't support abort signals directly,
            // but we keep this pattern for future compatibility
            return await fn(this.httpClient, headers);
        }
        finally {
            clearTimeout(timeoutId);
        }
    }
    isRetryableStatus(status) {
        return [408, 429, 502, 503, 504].includes(status);
    }
    delay(ms) {
        return new Promise((resolve) => setTimeout(resolve, ms));
    }
}
