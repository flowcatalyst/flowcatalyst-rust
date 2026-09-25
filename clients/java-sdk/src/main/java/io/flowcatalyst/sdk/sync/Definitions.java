package io.flowcatalyst.sdk.sync;

import com.fasterxml.jackson.annotation.JsonIgnore;
import com.fasterxml.jackson.annotation.JsonInclude;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.Map;

/**
 * Definition types for syncing FlowCatalyst primitives to the platform:
 * the roles an application needs, the event types it publishes, the
 * subscriptions it consumes, the dispatch pools it expects, the principals it
 * manages, its process documentation and scheduled jobs.
 *
 * <p>Build a {@link DefinitionSet} (one per application) via
 * {@link DefinitionSet#define} and pass it to
 * {@code client.definitions().sync(...)}. Mirrors the TypeScript SDK's
 * {@code sync/definitions.ts}.
 */
public final class Definitions {

    private Definitions() {}

    /**
     * A permission reference on a role: either an already-formatted 4-part
     * string ({@link #raw}) or a structured {@link Permission} whose
     * application segment defaults to the set's application code.
     */
    public sealed interface PermissionRef permits Permission, RawPermission {
        /** Resolve to the full {@code application:context:aggregate:action} form, lower-cased. */
        String resolve(String defaultApplication);

        static PermissionRef raw(String value) {
            return new RawPermission(value);
        }
    }

    /** An already-formatted permission string. */
    public record RawPermission(String value) implements PermissionRef {
        @Override
        public String resolve(String defaultApplication) {
            return value.toLowerCase(Locale.ROOT);
        }
    }

    /**
     * A structured permission — the 4-part
     * {@code <application>:<context>:<aggregate>:<action>} identity, defined
     * once and linkable from any number of roles. {@code application} may be
     * null, defaulting to the application code of the {@link DefinitionSet}
     * it is resolved against. FlowCatalyst has no standalone "create
     * permission" — permissions reach the platform via the roles that grant
     * them; the standalone catalogue is client-side documentation/reuse.
     */
    public record Permission(
            String application, String context, String aggregate, String action, String description)
            implements PermissionRef {

        public static Permission of(String context, String aggregate, String action) {
            return new Permission(null, context, aggregate, action, null);
        }

        public Permission withApplication(String application) {
            return new Permission(application, context, aggregate, action, description);
        }

        public Permission withDescription(String description) {
            return new Permission(application, context, aggregate, action, description);
        }

        @Override
        public String resolve(String defaultApplication) {
            String app = application != null ? application : defaultApplication;
            if (app == null) {
                throw new IllegalArgumentException(
                        "permission requires an application: set `application` on the permission "
                                + "or resolve it against a DefinitionSet/application code.");
            }
            return (app + ":" + context + ":" + aggregate + ":" + action).toLowerCase(Locale.ROOT);
        }
    }

    /**
     * A role declaration. Names are stored with the application code prefix:
     * given name {@code admin} under application {@code orders}, the role is
     * persisted as {@code orders:admin} — do not include the prefix yourself.
     */
    @JsonInclude(JsonInclude.Include.NON_NULL)
    public record Role(
            String name,
            String displayName,
            String description,
            List<PermissionRef> permissions,
            Boolean clientManaged) {

        public static Role of(String name) {
            return new Role(name, null, null, null, null);
        }

        public Role withDisplayName(String displayName) {
            return new Role(name, displayName, description, permissions, clientManaged);
        }

        public Role withDescription(String description) {
            return new Role(name, displayName, description, permissions, clientManaged);
        }

        public Role withPermissions(List<PermissionRef> permissions) {
            return new Role(name, displayName, description, permissions, clientManaged);
        }

        /**
         * When true, client admins can assign this role to their own users;
         * when false, only platform admins can.
         */
        public Role withClientManaged(boolean clientManaged) {
            return new Role(name, displayName, description, permissions, clientManaged);
        }
    }

    /**
     * An event type declaration. {@code code} is the full 4-part identifier
     * {@code <app>:<subdomain>:<aggregate>:<event>}; the first segment MUST
     * match the application code being synced. JSON Schema is not sync'd via
     * this endpoint — attach schemas through the admin UI or
     * {@code eventTypes().addSchemaVersion(...)}.
     */
    @JsonInclude(JsonInclude.Include.NON_NULL)
    public record EventType(String code, String name, String description) {
        public static EventType of(String code, String name) {
            return new EventType(code, name, null);
        }

        public EventType withDescription(String description) {
            return new EventType(code, name, description);
        }
    }

    /** How dispatch job failures interact with a subscription's delivery order. */
    public enum SubscriptionMode {
        /** Deliver independently; failures don't block other deliveries. */
        IMMEDIATE,
        /** On failure, hold subsequent deliveries for the same message group. */
        BLOCK_ON_ERROR
    }

    /** A single event-type binding inside a subscription. */
    @JsonInclude(JsonInclude.Include.NON_NULL)
    public record SubscriptionEventType(String eventTypeCode, String filter) {
        public static SubscriptionEventType of(String eventTypeCode) {
            return new SubscriptionEventType(eventTypeCode, null);
        }
    }

    /**
     * A subscription declaration: where to deliver ({@code target} URL or
     * {@code connectionCode} reference), which event types trigger it, and how
     * to handle failures.
     *
     * <p>{@code client} and {@code sharedConnection} are resolved by {@link
     * DefinitionSynchronizer} to pick which platform call this row belongs to
     * and which connection namespace {@code connectionCode} resolves in —
     * {@code client} never rides along on the wire (it is a routing field,
     * {@link JsonIgnore}d); {@code sharedConnection} is sent only when true.
     */
    @JsonInclude(JsonInclude.Include.NON_NULL)
    public record Subscription(
            String code,
            String name,
            String description,
            String target,
            String connectionId,
            List<SubscriptionEventType> eventTypes,
            String dispatchPoolCode,
            SubscriptionMode mode,
            Integer maxRetries,
            Integer timeoutSeconds,
            Boolean dataOnly,
            String connectionCode,
            Boolean sharedConnection,
            @JsonIgnore String client) {

        /**
         * The pre-{@code connectionCode} component list, kept so existing
         * callers of the canonical constructor keep compiling.
         */
        public Subscription(
                String code, String name, String description, String target, String connectionId,
                List<SubscriptionEventType> eventTypes, String dispatchPoolCode, SubscriptionMode mode,
                Integer maxRetries, Integer timeoutSeconds, Boolean dataOnly) {
            this(code, name, description, target, connectionId, eventTypes, dispatchPoolCode, mode,
                    maxRetries, timeoutSeconds, dataOnly, null);
        }

        /**
         * The pre-{@code sharedConnection}/{@code client} component list,
         * kept so existing callers of the (post-bb1b483) canonical
         * constructor keep compiling.
         */
        public Subscription(
                String code, String name, String description, String target, String connectionId,
                List<SubscriptionEventType> eventTypes, String dispatchPoolCode, SubscriptionMode mode,
                Integer maxRetries, Integer timeoutSeconds, Boolean dataOnly, String connectionCode) {
            this(code, name, description, target, connectionId, eventTypes, dispatchPoolCode, mode,
                    maxRetries, timeoutSeconds, dataOnly, connectionCode, null, null);
        }

        public static Subscription of(
                String code, String name, String target, List<SubscriptionEventType> eventTypes) {
            return new Subscription(
                    code, name, null, target, null, eventTypes, null, null, null, null, null, null,
                    null, null);
        }

        public Subscription withDescription(String description) {
            return new Subscription(code, name, description, target, connectionId, eventTypes,
                    dispatchPoolCode, mode, maxRetries, timeoutSeconds, dataOnly, connectionCode,
                    sharedConnection, client);
        }

        public Subscription withConnectionId(String connectionId) {
            return new Subscription(code, name, description, target, connectionId, eventTypes,
                    dispatchPoolCode, mode, maxRetries, timeoutSeconds, dataOnly, connectionCode,
                    sharedConnection, client);
        }

        /**
         * Names the connection by its code — stable across environments,
         * unlike {@link #withConnectionId}, whose id is minted per environment.
         * A bare code names a connection owned by THIS application; combine
         * with {@link #withSharedConnection} to name a shared one instead.
         * There is no fallback between the two namespaces.
         */
        public Subscription withConnectionCode(String connectionCode) {
            return new Subscription(code, name, description, target, connectionId, eventTypes,
                    dispatchPoolCode, mode, maxRetries, timeoutSeconds, dataOnly, connectionCode,
                    sharedConnection, client);
        }

        public Subscription withDispatchPoolCode(String dispatchPoolCode) {
            return new Subscription(code, name, description, target, connectionId, eventTypes,
                    dispatchPoolCode, mode, maxRetries, timeoutSeconds, dataOnly, connectionCode,
                    sharedConnection, client);
        }

        public Subscription withMode(SubscriptionMode mode) {
            return new Subscription(code, name, description, target, connectionId, eventTypes,
                    dispatchPoolCode, mode, maxRetries, timeoutSeconds, dataOnly, connectionCode,
                    sharedConnection, client);
        }

        public Subscription withMaxRetries(int maxRetries) {
            return new Subscription(code, name, description, target, connectionId, eventTypes,
                    dispatchPoolCode, mode, maxRetries, timeoutSeconds, dataOnly, connectionCode,
                    sharedConnection, client);
        }

        public Subscription withTimeoutSeconds(int timeoutSeconds) {
            return new Subscription(code, name, description, target, connectionId, eventTypes,
                    dispatchPoolCode, mode, maxRetries, timeoutSeconds, dataOnly, connectionCode,
                    sharedConnection, client);
        }

        /** When true, only the event's {@code data} field is POSTed (no metadata envelope). */
        public Subscription withDataOnly(boolean dataOnly) {
            return new Subscription(code, name, description, target, connectionId, eventTypes,
                    dispatchPoolCode, mode, maxRetries, timeoutSeconds, dataOnly, connectionCode,
                    sharedConnection, client);
        }

        /**
         * Marks {@link #connectionCode} as naming a SHARED (application-less)
         * connection rather than one owned by this application — the two
         * namespaces are distinct with no fallback between them. Normalises
         * {@code false} to {@code null} so the field is omitted from the
         * wire payload entirely when not true (the platform default).
         */
        public Subscription withSharedConnection(boolean sharedConnection) {
            return new Subscription(code, name, description, target, connectionId, eventTypes,
                    dispatchPoolCode, mode, maxRetries, timeoutSeconds, dataOnly, connectionCode,
                    sharedConnection ? Boolean.TRUE : null, client);
        }

        /**
         * FlowCatalyst client (identifier slug) this subscription is scoped
         * to. Null means this row inherits the owning {@link
         * DefinitionSet#forClient} client, or is global if the set doesn't
         * set one either. Routing only — {@link DefinitionSynchronizer} reads
         * it to pick which platform call the row belongs to; it never rides
         * along in the posted entry itself.
         */
        public Subscription withClient(String client) {
            return new Subscription(code, name, description, target, connectionId, eventTypes,
                    dispatchPoolCode, mode, maxRetries, timeoutSeconds, dataOnly, connectionCode,
                    sharedConnection, client);
        }
    }

    /**
     * A connection declaration. A connection is application-owned: the
     * platform assigns its service account itself (the application's own
     * provisioned account), so this definition carries nothing
     * environment-specific — no service account id, no secret. It exists
     * purely to give a subscription's {@code connectionCode} something to
     * resolve, and is synced BEFORE subscriptions for that reason.
     *
     * <p>{@code client} is resolved by {@link DefinitionSynchronizer} to pick
     * which platform call this row belongs to; it never rides along on the
     * wire (it is a routing field, {@link JsonIgnore}d).
     */
    @JsonInclude(JsonInclude.Include.NON_NULL)
    public record Connection(String code, String name, String description, String externalId,
            @JsonIgnore String client) {

        public static Connection of(String code, String name) {
            return new Connection(code, name, null, null, null);
        }

        public Connection withDescription(String description) {
            return new Connection(code, name, description, externalId, client);
        }

        /** Your own system's identifier for this connection, if any. */
        public Connection withExternalId(String externalId) {
            return new Connection(code, name, description, externalId, client);
        }

        /**
         * FlowCatalyst client (identifier slug) this connection is scoped
         * to — ALWAYS the identifier, never the id (ids differ per
         * environment). Null means this row inherits the owning {@link
         * DefinitionSet#forClient} client, or is global if the set doesn't
         * set one either. Routing only — never sent to the platform (it
         * selects which sync call this row belongs to).
         */
        public Connection withClient(String client) {
            return new Connection(code, name, description, externalId, client);
        }
    }

    /**
     * A dispatch pool declaration — concurrency cap and per-minute rate limit
     * for outbound delivery.
     */
    @JsonInclude(JsonInclude.Include.NON_NULL)
    public record DispatchPool(
            String code, String name, String description, Integer rateLimit, Integer concurrency) {

        public static DispatchPool of(String code, String name) {
            return new DispatchPool(code, name, null, null, null);
        }

        public DispatchPool withDescription(String description) {
            return new DispatchPool(code, name, description, rateLimit, concurrency);
        }

        /** Rate limit in requests per minute; platform default 100. */
        public DispatchPool withRateLimit(int rateLimit) {
            return new DispatchPool(code, name, description, rateLimit, concurrency);
        }

        /** Concurrency cap; platform default 10. */
        public DispatchPool withConcurrency(int concurrency) {
            return new DispatchPool(code, name, description, rateLimit, concurrency);
        }
    }

    /**
     * A principal (user) declaration, matched by email. {@code roles} lists
     * role short names WITHOUT the application prefix (the platform adds
     * {@code <app>:} per role).
     */
    @JsonInclude(JsonInclude.Include.NON_NULL)
    public record Principal(String email, String name, List<String> roles, Boolean active) {
        public static Principal of(String email, String name) {
            return new Principal(email, name, null, null);
        }

        public Principal withRoles(List<String> roles) {
            return new Principal(email, name, roles, active);
        }

        public Principal withActive(boolean active) {
            return new Principal(email, name, roles, active);
        }
    }

    /**
     * A process documentation declaration. {@code code} is the three-segment
     * identifier {@code <app>:<subdomain>:<process>}; {@code body} carries the
     * diagram source verbatim (typically Mermaid).
     */
    @JsonInclude(JsonInclude.Include.NON_NULL)
    public record Process(
            String code,
            String name,
            String description,
            String body,
            String diagramType,
            List<String> tags) {

        public static Process of(String code, String name) {
            return new Process(code, name, null, null, null, null);
        }

        public Process withDescription(String description) {
            return new Process(code, name, description, body, diagramType, tags);
        }

        public Process withBody(String body) {
            return new Process(code, name, description, body, diagramType, tags);
        }

        /** Diagram language; platform applies {@code mermaid} when omitted. */
        public Process withDiagramType(String diagramType) {
            return new Process(code, name, description, body, diagramType, tags);
        }

        public Process withTags(List<String> tags) {
            return new Process(code, name, description, body, diagramType, tags);
        }
    }

    /**
     * A scheduled-job declaration. {@code crons} requires 6-field,
     * seconds-first cron expressions ({@code sec min hour dom month dow}) — a
     * standard 5-field cron passes validation but never fires. {@code clientId}
     * scopes the job to a client/tenant; omit it only for platform-wide jobs
     * (anchor-only).
     */
    @JsonInclude(JsonInclude.Include.NON_NULL)
    public record ScheduledJob(
            String code,
            String name,
            String description,
            List<String> crons,
            String timezone,
            Object payload,
            Boolean concurrent,
            Boolean tracksCompletion,
            Integer timeoutSeconds,
            Integer deliveryMaxAttempts,
            String targetUrl,
            String clientId) {

        public static ScheduledJob of(String code, String name, List<String> crons) {
            return new ScheduledJob(code, name, null, crons, null, null, null, null, null, null,
                    null, null);
        }

        public ScheduledJob withDescription(String description) {
            return new ScheduledJob(code, name, description, crons, timezone, payload, concurrent,
                    tracksCompletion, timeoutSeconds, deliveryMaxAttempts, targetUrl, clientId);
        }

        public ScheduledJob withTimezone(String timezone) {
            return new ScheduledJob(code, name, description, crons, timezone, payload, concurrent,
                    tracksCompletion, timeoutSeconds, deliveryMaxAttempts, targetUrl, clientId);
        }

        public ScheduledJob withPayload(Object payload) {
            return new ScheduledJob(code, name, description, crons, timezone, payload, concurrent,
                    tracksCompletion, timeoutSeconds, deliveryMaxAttempts, targetUrl, clientId);
        }

        /** Most apps want false — allows a new tick while a previous invocation still runs. */
        public ScheduledJob withConcurrent(boolean concurrent) {
            return new ScheduledJob(code, name, description, crons, timezone, payload, concurrent,
                    tracksCompletion, timeoutSeconds, deliveryMaxAttempts, targetUrl, clientId);
        }

        /**
         * When true, the consumer must POST back to
         * {@code /api/scheduled-jobs/instances/{id}/complete}, enabling
         * per-instance status tracking.
         */
        public ScheduledJob withTracksCompletion(boolean tracksCompletion) {
            return new ScheduledJob(code, name, description, crons, timezone, payload, concurrent,
                    tracksCompletion, timeoutSeconds, deliveryMaxAttempts, targetUrl, clientId);
        }

        public ScheduledJob withTimeoutSeconds(int timeoutSeconds) {
            return new ScheduledJob(code, name, description, crons, timezone, payload, concurrent,
                    tracksCompletion, timeoutSeconds, deliveryMaxAttempts, targetUrl, clientId);
        }

        public ScheduledJob withDeliveryMaxAttempts(int deliveryMaxAttempts) {
            return new ScheduledJob(code, name, description, crons, timezone, payload, concurrent,
                    tracksCompletion, timeoutSeconds, deliveryMaxAttempts, targetUrl, clientId);
        }

        /** Override the application's default callback URL for this job. */
        public ScheduledJob withTargetUrl(String targetUrl) {
            return new ScheduledJob(code, name, description, crons, timezone, payload, concurrent,
                    tracksCompletion, timeoutSeconds, deliveryMaxAttempts, targetUrl, clientId);
        }

        /** Client/tenant that owns this job; null = platform-scoped (anchor only). */
        public ScheduledJob withClientId(String clientId) {
            return new ScheduledJob(code, name, description, crons, timezone, payload, concurrent,
                    tracksCompletion, timeoutSeconds, deliveryMaxAttempts, targetUrl, clientId);
        }
    }

    /** Container for all definitions belonging to one application. */
    public static final class DefinitionSet {
        private final String applicationCode;
        private final List<Role> roles = new ArrayList<>();
        private final List<Permission> permissions = new ArrayList<>();
        private final List<EventType> eventTypes = new ArrayList<>();
        private final List<Subscription> subscriptions = new ArrayList<>();
        private final List<Connection> connections = new ArrayList<>();
        private final List<DispatchPool> dispatchPools = new ArrayList<>();
        private final List<Principal> principals = new ArrayList<>();
        private final List<Process> processes = new ArrayList<>();
        private final List<ScheduledJob> scheduledJobs = new ArrayList<>();
        private Map<String, Object> openapiSpec;
        private String client;
        private String targetBaseUrl;

        private DefinitionSet(String applicationCode) {
            this.applicationCode = applicationCode;
        }

        /** Environment variable read by {@link #defineFromEnv()}. */
        public static final String APP_CODE_ENV = "FLOWCATALYST_APP_CODE";

        /** Start building definitions for {@code applicationCode}. */
        public static DefinitionSet define(String applicationCode) {
            return new DefinitionSet(applicationCode);
        }

        /**
         * Start building definitions for the application named by
         * {@code FLOWCATALYST_APP_CODE}, for apps that carry their code in the
         * environment rather than in source.
         *
         * <p>A codebase that owns several applications should call
         * {@link #define(String)} once per application and pass the sets to
         * {@code definitions().syncAll(…)} — the set a definition belongs to
         * <em>is</em> its application.
         *
         * @throws IllegalStateException if the variable is unset or blank; a
         *     missing code would otherwise surface later as a request to
         *     {@code /api/applications/null/…}
         */
        public static DefinitionSet defineFromEnv() {
            String applicationCode = System.getenv(APP_CODE_ENV);
            if (applicationCode == null || applicationCode.isBlank()) {
                throw new IllegalStateException(
                        APP_CODE_ENV + " is not set — pass the application code to define(…)"
                                + " instead.");
            }
            return new DefinitionSet(applicationCode);
        }

        public DefinitionSet withRoles(List<Role> roles) {
            this.roles.addAll(roles);
            return this;
        }

        /**
         * Declare standalone permissions (reusable across roles). Their
         * application segment defaults to this set's applicationCode.
         */
        public DefinitionSet withPermissions(List<Permission> permissions) {
            this.permissions.addAll(permissions);
            return this;
        }

        public DefinitionSet withEventTypes(List<EventType> eventTypes) {
            this.eventTypes.addAll(eventTypes);
            return this;
        }

        public DefinitionSet withSubscriptions(List<Subscription> subscriptions) {
            this.subscriptions.addAll(subscriptions);
            return this;
        }

        /**
         * Add connections to the definition set. Synced BEFORE subscriptions
         * — a subscription's {@code connectionCode} must resolve in the same
         * run.
         */
        public DefinitionSet withConnections(List<Connection> connections) {
            this.connections.addAll(connections);
            return this;
        }

        public List<Connection> connections() {
            return connections;
        }

        /**
         * Scope this set to a FlowCatalyst client (identifier slug — never an
         * id; ids differ per environment). This is the multi-tenant shape:
         * build one {@link DefinitionSet} per (application, client) — the
         * plain {@link #define} set stays global, and one more set per
         * tenant via {@code define(app).forClient(tenant)}. The tenant list
         * is your own runtime data; it never belongs in an annotation.
         *
         * <p>A connection or subscription row's own {@code client} (set via
         * the annotation or {@code withClient(...)}) wins over this; this is
         * only the fallback applied to rows that don't set one themselves.
         *
         * @return this set, now scoped to {@code client}
         */
        public DefinitionSet forClient(String client) {
            return forClient(client, null);
        }

        /**
         * As {@link #forClient(String)}, additionally overriding the base
         * URL a path-style subscription target ({@code /webhooks/orders})
         * resolves against for THIS set only — a tenant often has its own
         * host. Null keeps {@link DefinitionSynchronizer}'s configured
         * default.
         */
        public DefinitionSet forClient(String client, String targetBaseUrl) {
            this.client = client;
            this.targetBaseUrl = targetBaseUrl;
            return this;
        }

        /** The client (identifier slug) this set is scoped to, or null for global. */
        public String client() {
            return client;
        }

        /** This set's subscription-target base URL override, or null. */
        public String targetBaseUrl() {
            return targetBaseUrl;
        }

        public DefinitionSet withDispatchPools(List<DispatchPool> pools) {
            this.dispatchPools.addAll(pools);
            return this;
        }

        public DefinitionSet withPrincipals(List<Principal> principals) {
            this.principals.addAll(principals);
            return this;
        }

        public DefinitionSet withProcesses(List<Process> processes) {
            this.processes.addAll(processes);
            return this;
        }

        public DefinitionSet withScheduledJobs(List<ScheduledJob> jobs) {
            this.scheduledJobs.addAll(jobs);
            return this;
        }

        /**
         * Attach an OpenAPI document (parsed JSON) to publish alongside the
         * rest of the application's definitions. Each sync replaces the
         * previously published version.
         */
        public DefinitionSet withOpenapiSpec(Map<String, Object> spec) {
            this.openapiSpec = spec;
            return this;
        }

        public String applicationCode() {
            return applicationCode;
        }

        public List<Role> roles() {
            return roles;
        }

        public List<Permission> permissions() {
            return permissions;
        }

        public List<EventType> eventTypes() {
            return eventTypes;
        }

        public List<Subscription> subscriptions() {
            return subscriptions;
        }

        public List<DispatchPool> dispatchPools() {
            return dispatchPools;
        }

        public List<Principal> principals() {
            return principals;
        }

        public List<Process> processes() {
            return processes;
        }

        public List<ScheduledJob> scheduledJobs() {
            return scheduledJobs;
        }

        public Map<String, Object> openapiSpec() {
            return openapiSpec;
        }
    }
}
