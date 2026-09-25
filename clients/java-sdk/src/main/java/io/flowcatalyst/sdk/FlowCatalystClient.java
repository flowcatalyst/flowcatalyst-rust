package io.flowcatalyst.sdk;

import io.flowcatalyst.sdk.auth.ClientCredentialsTokenManager;
import io.flowcatalyst.sdk.auth.TokenProvider;
import io.flowcatalyst.sdk.http.Transport;
import io.flowcatalyst.sdk.resources.ApplicationsResource;
import io.flowcatalyst.sdk.resources.AuditLogsResource;
import io.flowcatalyst.sdk.resources.ClientsResource;
import io.flowcatalyst.sdk.resources.ConnectionsResource;
import io.flowcatalyst.sdk.resources.DispatchPoolsResource;
import io.flowcatalyst.sdk.resources.EventTypesResource;
import io.flowcatalyst.sdk.resources.MeResource;
import io.flowcatalyst.sdk.resources.PermissionsResource;
import io.flowcatalyst.sdk.resources.PrincipalsResource;
import io.flowcatalyst.sdk.resources.ProcessesResource;
import io.flowcatalyst.sdk.resources.RolesResource;
import io.flowcatalyst.sdk.resources.RouterResource;
import io.flowcatalyst.sdk.resources.ScheduledJobsResource;
import io.flowcatalyst.sdk.resources.SubscriptionsResource;
import java.time.Duration;
import java.util.Objects;
import java.util.function.Supplier;

/**
 * Main FlowCatalyst SDK client. Mirrors the TypeScript SDK: two auth modes
 * (OAuth2 client credentials, or a caller-supplied user token), retry with
 * exponential backoff on transient errors, and typed failures via
 * {@link io.flowcatalyst.sdk.error.FlowCatalystException}.
 *
 * <pre>{@code
 * var client = FlowCatalystClient.builder()
 *         .baseUrl("https://your-instance.flowcatalyst.io")
 *         .clientCredentials("your_client_id", "your_client_secret")
 *         .build();
 *
 * var eventTypes = client.eventTypes().list(null);
 * }</pre>
 */
public final class FlowCatalystClient {

    private final Transport transport;
    private final String routerBaseUrl;
    private final String subscriptionTargetBaseUrl;
    private final ClientCredentialsTokenManager tokenManager;

    private EventTypesResource eventTypes;
    private ProcessesResource processes;
    private SubscriptionsResource subscriptions;
    private DispatchPoolsResource dispatchPools;
    private RolesResource roles;
    private PermissionsResource permissions;
    private ApplicationsResource applications;
    private ClientsResource clients;
    private PrincipalsResource principals;
    private MeResource me;
    private ConnectionsResource connections;
    private RouterResource router;
    private ScheduledJobsResource scheduledJobs;
    private AuditLogsResource auditLogs;
    private io.flowcatalyst.sdk.sync.DefinitionSynchronizer definitions;

    private FlowCatalystClient(Builder builder) {
        String baseUrl = builder.baseUrl.replaceAll("/$", "");
        this.routerBaseUrl =
                (builder.routerBaseUrl != null ? builder.routerBaseUrl : builder.baseUrl)
                        .replaceAll("/$", "");
        this.subscriptionTargetBaseUrl = builder.subscriptionTargetBaseUrl;

        TokenProvider provider;
        if (builder.tokenProvider != null) {
            // User token mode — the host application owns refresh.
            this.tokenManager = null;
            provider = builder.tokenProvider;
        } else {
            this.tokenManager = new ClientCredentialsTokenManager(
                    baseUrl, builder.clientId, builder.clientSecret, builder.tokenUrl);
            provider = this.tokenManager;
        }

        this.transport = new Transport(
                baseUrl,
                provider,
                tokenManager,
                builder.timeout,
                builder.retryAttempts,
                builder.retryDelay);
    }

    public static Builder builder() {
        return new Builder();
    }

    // ── Resource accessors ──────────────────────────────────────────

    public synchronized EventTypesResource eventTypes() {
        return eventTypes != null ? eventTypes : (eventTypes = new EventTypesResource(transport));
    }

    /** Processes resource (workflow documentation / Mermaid diagrams). */
    public synchronized ProcessesResource processes() {
        return processes != null ? processes : (processes = new ProcessesResource(transport));
    }

    public synchronized SubscriptionsResource subscriptions() {
        return subscriptions != null
                ? subscriptions : (subscriptions = new SubscriptionsResource(transport));
    }

    public synchronized DispatchPoolsResource dispatchPools() {
        return dispatchPools != null
                ? dispatchPools : (dispatchPools = new DispatchPoolsResource(transport));
    }

    public synchronized RolesResource roles() {
        return roles != null ? roles : (roles = new RolesResource(transport));
    }

    public synchronized PermissionsResource permissions() {
        return permissions != null ? permissions : (permissions = new PermissionsResource(transport));
    }

    public synchronized ApplicationsResource applications() {
        return applications != null
                ? applications : (applications = new ApplicationsResource(transport));
    }

    public synchronized ClientsResource clients() {
        return clients != null ? clients : (clients = new ClientsResource(transport));
    }

    public synchronized PrincipalsResource principals() {
        return principals != null ? principals : (principals = new PrincipalsResource(transport));
    }

    /** Me resource (user-scoped access to clients and applications). */
    public synchronized MeResource me() {
        return me != null ? me : (me = new MeResource(transport));
    }

    public synchronized ConnectionsResource connections() {
        return connections != null ? connections : (connections = new ConnectionsResource(transport));
    }

    /**
     * Message router monitoring resource — presence checks against the
     * router's in-pipeline map. Talks to {@code routerBaseUrl} if configured,
     * otherwise falls back to {@code baseUrl}.
     */
    public synchronized RouterResource router() {
        return router != null ? router : (router = new RouterResource(transport, routerBaseUrl));
    }

    /** Scheduled Jobs resource (CRUD + state transitions + history reads). */
    public synchronized ScheduledJobsResource scheduledJobs() {
        return scheduledJobs != null
                ? scheduledJobs : (scheduledJobs = new ScheduledJobsResource(transport));
    }

    /** Audit Logs resource (read-only queries). */
    public synchronized AuditLogsResource auditLogs() {
        return auditLogs != null ? auditLogs : (auditLogs = new AuditLogsResource(transport));
    }

    /**
     * Definition synchronizer — bulk-sync roles, event types, connections,
     * subscriptions, dispatch pools, principals, processes, scheduled jobs,
     * and an OpenAPI doc per application.
     */
    public synchronized io.flowcatalyst.sdk.sync.DefinitionSynchronizer definitions() {
        return definitions != null
                ? definitions
                : (definitions = new io.flowcatalyst.sdk.sync.DefinitionSynchronizer(
                        transport, subscriptionTargetBaseUrl));
    }

    // ── Advanced access ─────────────────────────────────────────────

    /** The underlying transport (for advanced usage and SDK-internal modules). */
    public Transport transport() {
        return transport;
    }

    /** The token manager, or null in user-token mode. */
    public ClientCredentialsTokenManager tokenManager() {
        return tokenManager;
    }

    public String routerBaseUrl() {
        return routerBaseUrl;
    }

    // ── Builder ─────────────────────────────────────────────────────

    public static final class Builder {
        private String baseUrl;
        private String clientId;
        private String clientSecret;
        private String tokenUrl;
        private TokenProvider tokenProvider;
        private String routerBaseUrl;
        private String subscriptionTargetBaseUrl;
        private Duration timeout = Duration.ofSeconds(30);
        private int retryAttempts = 3;
        private Duration retryDelay = Duration.ofMillis(100);

        /** Base URL of the FlowCatalyst platform (required). */
        public Builder baseUrl(String baseUrl) {
            this.baseUrl = baseUrl;
            return this;
        }

        /** Authenticate with OAuth2 client credentials (service account). */
        public Builder clientCredentials(String clientId, String clientSecret) {
            this.clientId = clientId;
            this.clientSecret = clientSecret;
            return this;
        }

        /** Custom token endpoint (defaults to {@code {baseUrl}/oauth/token}). */
        public Builder tokenUrl(String tokenUrl) {
            this.tokenUrl = tokenUrl;
            return this;
        }

        /** Authenticate with a static user access token; 401s are not auto-refreshed. */
        public Builder accessToken(String accessToken) {
            this.tokenProvider = () -> accessToken;
            return this;
        }

        /**
         * Authenticate with a token supplier owned by the host application
         * (e.g. refreshed by your login flow); 401s are not auto-refreshed.
         */
        public Builder accessToken(Supplier<String> tokenSupplier) {
            this.tokenProvider = tokenSupplier::get;
            return this;
        }

        /**
         * Base URL of the message router for monitoring endpoints. The router
         * runs at a different host than the platform; if unset, router calls
         * fall back to {@code baseUrl} (correct only when router and platform
         * share a host, e.g. fcdev).
         */
        public Builder routerBaseUrl(String routerBaseUrl) {
            this.routerBaseUrl = routerBaseUrl;
            return this;
        }

        /**
         * Default base URL a subscription's path-style sync target
         * ({@code /webhooks/orders}) resolves against, used by {@code
         * definitions().sync(...)} when neither the row nor its {@link
         * io.flowcatalyst.sdk.sync.Definitions.DefinitionSet#forClient}
         * override supplies one. Optional; a synchronizer built directly
         * (e.g. {@code new DefinitionSynchronizer(client.transport(), ...)})
         * can set a different one per instance.
         */
        public Builder subscriptionTargetBaseUrl(String subscriptionTargetBaseUrl) {
            this.subscriptionTargetBaseUrl = subscriptionTargetBaseUrl;
            return this;
        }

        /** Request timeout (default 30s). */
        public Builder timeout(Duration timeout) {
            this.timeout = timeout;
            return this;
        }

        /** Retry attempts for transient errors (default 3). */
        public Builder retryAttempts(int retryAttempts) {
            this.retryAttempts = retryAttempts;
            return this;
        }

        /** Base delay between retries, doubled per attempt (default 100ms). */
        public Builder retryDelay(Duration retryDelay) {
            this.retryDelay = retryDelay;
            return this;
        }

        public FlowCatalystClient build() {
            Objects.requireNonNull(baseUrl, "baseUrl is required");
            if (tokenProvider == null && (clientId == null || clientSecret == null)) {
                throw new IllegalStateException(
                        "Configure clientCredentials(...) or accessToken(...)");
            }
            return new FlowCatalystClient(this);
        }
    }
}
