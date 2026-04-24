/**
 * FlowCatalyst SDK Client
 *
 * Main client for interacting with the FlowCatalyst platform.
 * Uses neverthrow for typed error handling.
 */

import { ResultAsync, ok, err, errAsync } from "neverthrow";
import { createClient, createConfig } from "./generated/client";
import type { Client } from "./generated/client";
import { OidcTokenManager, type TokenManagerConfig } from "./auth";
import type { SdkError } from "./errors";
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

function isUserTokenConfig(
	config: FlowCatalystConfig,
): config is UserTokenConfig {
	return "accessToken" in config;
}

type ResolvedConfig = {
	baseUrl: string;
	timeout: number;
	retryAttempts: number;
	retryDelay: number;
};

type TokenProvider = () => Promise<string>;

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
	private readonly httpClient: Client;
	private readonly tokenManager: OidcTokenManager | null;
	private readonly tokenProvider: TokenProvider | null;
	private readonly config: ResolvedConfig;

	// Lazy-loaded resource instances
	private _eventTypes?: EventTypesResource;
	private _subscriptions?: SubscriptionsResource;
	private _dispatchPools?: DispatchPoolsResource;
	private _roles?: RolesResource;
	private _permissions?: PermissionsResource;
	private _applications?: ApplicationsResource;
	private _clients?: ClientsResource;
	private _principals?: PrincipalsResource;
	private _me?: MeResource;
	private _connections?: ConnectionsResource;
	private _definitions?: DefinitionSynchronizer;

	constructor(config: FlowCatalystConfig) {
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
			} else {
				// Function that returns token (sync or async)
				this.tokenProvider = async () => tokenOrFn();
			}
		} else {
			// Client credentials mode - use token manager
			this.tokenManager = new OidcTokenManager({
				baseUrl: this.config.baseUrl,
				clientId: config.clientId,
				clientSecret: config.clientSecret,
				tokenUrl: config.tokenUrl,
			});
			this.tokenProvider = null;
		}

		this.httpClient = createClient(
			createConfig({
				baseUrl: this.config.baseUrl,
			}),
		);
	}

	// ============ Resource Accessors ============

	/** Event Types resource */
	eventTypes(): EventTypesResource {
		return (this._eventTypes ??= new EventTypesResource(this));
	}

	/** Subscriptions resource */
	subscriptions(): SubscriptionsResource {
		return (this._subscriptions ??= new SubscriptionsResource(this));
	}

	/** Dispatch Pools resource */
	dispatchPools(): DispatchPoolsResource {
		return (this._dispatchPools ??= new DispatchPoolsResource(this));
	}

	/** Roles resource */
	roles(): RolesResource {
		return (this._roles ??= new RolesResource(this));
	}

	/** Permissions resource */
	permissions(): PermissionsResource {
		return (this._permissions ??= new PermissionsResource(this));
	}

	/** Applications resource */
	applications(): ApplicationsResource {
		return (this._applications ??= new ApplicationsResource(this));
	}

	/** Clients resource */
	clients(): ClientsResource {
		return (this._clients ??= new ClientsResource(this));
	}

	/** Principals resource */
	principals(): PrincipalsResource {
		return (this._principals ??= new PrincipalsResource(this));
	}

	/** Me resource (user-scoped access to clients and applications) */
	me(): MeResource {
		return (this._me ??= new MeResource(this));
	}

	/** Connections resource */
	connections(): ConnectionsResource {
		return (this._connections ??= new ConnectionsResource(this));
	}

	/**
	 * Definition synchronizer — bulk-sync roles, event types, subscriptions,
	 * dispatch pools, and principals per application.
	 *
	 * See `docs/syncing-definitions.md` for structure and conventions.
	 */
	definitions(): DefinitionSynchronizer {
		return (this._definitions ??= new DefinitionSynchronizer(this));
	}

	// ============ Internal Methods ============

	/**
	 * Get the underlying HTTP client (for advanced usage).
	 */
	getHttpClient(): Client {
		return this.httpClient;
	}

	/**
	 * Get the token manager (for advanced usage).
	 * Returns null if using user token mode.
	 */
	getTokenManager(): OidcTokenManager | null {
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
	request<T>(
		fn: (
			client: Client,
			headers: Record<string, string>,
		) => Promise<{ data?: unknown; error?: unknown; response: Response }>,
	): ResultAsync<T, SdkError> {
		if (this.tokenProvider) {
			// User token mode
			return ResultAsync.fromPromise(this.tokenProvider(), (e) =>
				authError.tokenExpired(
					e instanceof Error ? e.message : "Failed to get access token",
				),
			).andThen((token) => this.executeWithRetry<T>(fn, token, 0, false));
		} else if (this.tokenManager) {
			// Client credentials mode
			return this.tokenManager
				.getAccessToken()
				.mapErr((e): SdkError => e)
				.andThen((token) => this.executeWithRetry<T>(fn, token, 0, true));
		} else {
			return errAsync(authError.tokenExpired("No authentication configured"));
		}
	}

	private executeWithRetry<T>(
		fn: (
			client: Client,
			headers: Record<string, string>,
		) => Promise<{ data?: unknown; error?: unknown; response: Response }>,
		token: string,
		attempt: number,
		canRefreshToken: boolean,
	): ResultAsync<T, SdkError> {
		const headers = {
			Authorization: `Bearer ${token}`,
		};

		return ResultAsync.fromPromise(
			this.executeWithTimeout(fn, headers),
			(e) => {
				if (e instanceof Error && e.name === "AbortError") {
					return httpError.timeout(this.config.timeout);
				}
				return httpError.network(
					e instanceof Error ? e.message : "Network error",
					e instanceof Error ? e : undefined,
				);
			},
		).andThen((result) => {
			// Check if we got an error response
			if (result.error !== undefined || !result.response.ok) {
				const status = result.response.status;

				// Handle 401 with token refresh (only for client credentials mode)
				if (
					status === 401 &&
					attempt === 0 &&
					canRefreshToken &&
					this.tokenManager
				) {
					return this.tokenManager
						.refreshToken()
						.mapErr((e): SdkError => e)
						.andThen((newToken) =>
							this.executeWithRetry<T>(
								fn,
								newToken,
								attempt + 1,
								canRefreshToken,
							),
						);
				}

				// Retry transient errors
				if (
					this.isRetryableStatus(status) &&
					attempt < this.config.retryAttempts
				) {
					return ResultAsync.fromSafePromise<void, SdkError>(
						this.delay(this.config.retryDelay * Math.pow(2, attempt)),
					).andThen(() =>
						this.executeWithRetry<T>(fn, token, attempt + 1, canRefreshToken),
					);
				}

				const sdkError = mapHttpStatusToError(status, result.error);
				return err(sdkError);
			}

			return ok(result.data as T);
		});
	}

	private async executeWithTimeout(
		fn: (
			client: Client,
			headers: Record<string, string>,
		) => Promise<{ data?: unknown; error?: unknown; response: Response }>,
		headers: Record<string, string>,
	): Promise<{ data?: unknown; error?: unknown; response: Response }> {
		const controller = new AbortController();
		const timeoutId = setTimeout(() => controller.abort(), this.config.timeout);

		try {
			// Note: The generated client doesn't support abort signals directly,
			// but we keep this pattern for future compatibility
			return await fn(this.httpClient, headers);
		} finally {
			clearTimeout(timeoutId);
		}
	}

	private isRetryableStatus(status: number): boolean {
		return [408, 429, 502, 503, 504].includes(status);
	}

	private delay(ms: number): Promise<void> {
		return new Promise((resolve) => setTimeout(resolve, ms));
	}
}
