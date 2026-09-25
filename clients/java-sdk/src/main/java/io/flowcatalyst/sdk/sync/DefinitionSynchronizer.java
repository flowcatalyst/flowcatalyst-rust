package io.flowcatalyst.sdk.sync;

import io.flowcatalyst.sdk.error.FlowCatalystException;
import io.flowcatalyst.sdk.http.Transport;
import io.flowcatalyst.sdk.sync.Definitions.DefinitionSet;
import io.flowcatalyst.sdk.sync.SyncOptions.SyncCategory;
import io.flowcatalyst.sdk.sync.SyncResult.Category;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.function.Function;
import java.util.regex.Pattern;

/**
 * DefinitionSynchronizer — orchestrates syncing a {@link DefinitionSet} to
 * the platform's application-scoped sync API
 * ({@code /api/applications/{app}/*}{@code /sync}).
 *
 * <p>Categories are sync'd in a fixed order — roles, event types,
 * connections, subscriptions, dispatch pools, principals, processes,
 * scheduled jobs, OpenAPI — so that subscriptions can reference the
 * connections, event types and dispatch pools that were just created. Each
 * category sync is an independent HTTP call; a failure in one does NOT roll
 * back earlier successes.
 *
 * <p>Connections and subscriptions are additionally scoped by client: the
 * platform treats each {@code (application, client)} sync call as the
 * COMPLETE list for that scope and, with {@code removeUnlisted}, deletes
 * everything else in it. A {@link DefinitionSet} may therefore contain rows
 * for several clients (via each row's own {@code client}, or the whole set's
 * {@link DefinitionSet#forClient}); they are grouped into one platform call
 * per distinct client — global first — connections before subscriptions
 * within each. If a scope's connection sync fails, that scope's subscription
 * sync is skipped and reported as an error rather than sent as a request
 * that cannot resolve its connections.
 *
 * <p>{@link #sync} and {@link #syncAll} keep their single-set behaviour —
 * they do NOT merge sets. {@link #syncGrouped} MERGES every set sharing an
 * application code into one combined sync before calling the platform,
 * because two sets syncing the SAME (application, client) scope separately
 * would let the second call's {@code removeUnlisted} delete what the first
 * call just created.
 *
 * <p>If ANY category of ANY application ends up {@link Category.Failed},
 * the call throws {@link DefinitionSyncException} rather than returning
 * normally — a caller that doesn't inspect every category must not be able
 * to mistake a partial failure for success. {@link #syncAll} and {@link
 * #syncGrouped} still run every set/application to completion first and
 * throw once at the end, carrying every result (including the ones that DID
 * sync); see {@link DefinitionSyncException} for how to read the partial
 * outcome.
 */
public final class DefinitionSynchronizer {

    // Matches an absolute URL (has a scheme) as opposed to a bare path like
    // "/webhooks/orders" that needs a base URL to resolve against.
    private static final Pattern HAS_SCHEME = Pattern.compile("^[a-zA-Z][a-zA-Z0-9+.-]*://");

    private final Transport transport;
    private final String subscriptionTargetBaseUrl;

    public DefinitionSynchronizer(Transport transport) {
        this(transport, null);
    }

    /**
     * @param subscriptionTargetBaseUrl Base URL a subscription's path-style
     *        target ({@code /webhooks/orders}) is resolved against when
     *        neither the row's own set (via {@link DefinitionSet#forClient})
     *        supplies one. Null means a path-style target with no other base
     *        available fails that subscription's sync locally, naming it,
     *        rather than being sent — under {@code removeUnlisted}, silently
     *        omitting it would delete it on the platform.
     */
    public DefinitionSynchronizer(Transport transport, String subscriptionTargetBaseUrl) {
        this.transport = transport;
        this.subscriptionTargetBaseUrl = subscriptionTargetBaseUrl;
    }

    /** Sync one application's definitions with default options. */
    public SyncResult sync(DefinitionSet set) {
        return sync(set, SyncOptions.defaults());
    }

    /**
     * Sync one application's definitions.
     *
     * @throws DefinitionSyncException if any category came back {@link
     *         Category.Failed} — {@link DefinitionSyncException#result()}
     *         carries the full result, including every category that DID
     *         sync
     */
    public SyncResult sync(DefinitionSet set, SyncOptions options) {
        return DefinitionSyncException.throwIfFailed(syncInternal(set, options));
    }

    /** The actual single-set sync; never throws for a {@link Category.Failed} — {@link #sync} does that. */
    private SyncResult syncInternal(DefinitionSet set, SyncOptions options) {
        String app = set.applicationCode();
        boolean removeUnlisted = options.removeUnlisted();

        Category roles = options.skips(SyncCategory.ROLES) || set.roles().isEmpty()
                ? Category.SKIPPED
                : syncRoles(app, set.roles(), removeUnlisted);
        Category eventTypes = options.skips(SyncCategory.EVENT_TYPES) || set.eventTypes().isEmpty()
                ? Category.SKIPPED
                : syncEventTypes(app, set.eventTypes(), removeUnlisted);

        ScopedResult scoped = syncConnectionsAndSubscriptions(
                app, toConnectionRows(set), toSubscriptionRows(set), options, subscriptionTargetBaseUrl);

        Category dispatchPools =
                options.skips(SyncCategory.DISPATCH_POOLS) || set.dispatchPools().isEmpty()
                        ? Category.SKIPPED
                        : syncDispatchPools(app, set.dispatchPools(), removeUnlisted);
        Category principals = options.skips(SyncCategory.PRINCIPALS) || set.principals().isEmpty()
                ? Category.SKIPPED
                : post(app, "principals", Map.of("principals", set.principals()), removeUnlisted);
        Category processes = options.skips(SyncCategory.PROCESSES) || set.processes().isEmpty()
                ? Category.SKIPPED
                : post(app, "processes", Map.of("processes", set.processes()), removeUnlisted);
        Category scheduledJobs =
                options.skips(SyncCategory.SCHEDULED_JOBS) || set.scheduledJobs().isEmpty()
                        ? Category.SKIPPED
                        : syncScheduledJobs(app, set.scheduledJobs(), removeUnlisted);
        Category openapi = options.skips(SyncCategory.OPENAPI) || set.openapiSpec() == null
                ? Category.SKIPPED
                : syncOpenapi(app, set.openapiSpec());

        return new SyncResult(app, roles, eventTypes, scoped.subscriptions(), dispatchPools, principals,
                processes, scheduledJobs, openapi, scoped.connections());
    }

    /**
     * Sync multiple applications' definitions sequentially; results are
     * returned in the same order.
     *
     * <p>Unlike {@link #syncGrouped}, sets are NOT merged — two sets for the
     * same application are synced as two separate calls. When they share an
     * (application, client) scope, the second call's {@code removeUnlisted}
     * will delete what the first just created; use {@link #syncGrouped} for
     * several sets contributing to one application.
     *
     * <p>Every set is synced before this can throw for a {@link
     * Category.Failed} — one set's duplicate code or failed connection sync
     * does not stop the rest from being attempted. A genuinely uncaught
     * exception (e.g. a network failure from a category that does not catch
     * its own — roles, event types, dispatch pools, principals, processes,
     * scheduled jobs, OpenAPI) still propagates immediately and stops the
     * run, exactly as it always has.
     *
     * @throws DefinitionSyncException if any set's any category came back
     *         {@link Category.Failed} — {@link
     *         DefinitionSyncException#results()} carries every set's result,
     *         in order, including the ones that synced fully
     */
    public List<SyncResult> syncAll(List<DefinitionSet> sets, SyncOptions options) {
        List<SyncResult> results = new ArrayList<>(sets.size());
        for (DefinitionSet set : sets) {
            results.add(syncInternal(set, options));
        }
        return DefinitionSyncException.throwIfAnyFailed(results);
    }

    /** {@link #syncGrouped(List, SyncOptions)} with default options. */
    public Map<String, SyncResult> syncGrouped(List<DefinitionSet> sets) {
        return syncGrouped(sets, SyncOptions.defaults());
    }

    /**
     * Sync multiple definition sets, grouping by application code and
     * MERGING every set that shares one into a single combined sync — then
     * issuing each category's platform call exactly ONCE per application
     * (and, for connections/subscriptions, once per (application, client)
     * scope within it).
     *
     * <p>This is not an optimisation: the platform scopes {@code
     * removeUnlisted} to one (application, client) PER CALL, so two sets for
     * the same application (e.g. scanned annotation definitions plus a
     * multi-tenant provider's per-tenant sets) must never become two
     * separate calls for that scope — the second would delete what the
     * first just created. Connections and subscriptions are stamped with
     * their effective client (their own {@code client}, else their owning
     * set's {@link DefinitionSet#forClient} client) before being pooled
     * across all contributing sets and grouped by that client — the same
     * grouping {@link #sync} already does for a single set.
     *
     * <p>The same code appearing twice in one (application, client) scope
     * after merging is a configuration error: that type's sync for that
     * scope fails LOCALLY, naming the code and the scope, and nothing is
     * sent for it — other types and other scopes still sync.
     *
     * <p>Every application is synced before this can throw for a {@link
     * Category.Failed} — one application's failure does not stop the rest
     * from being attempted. A genuinely uncaught exception (e.g. a network
     * failure from a category that does not catch its own) still propagates
     * immediately and stops the run, exactly as it always has.
     *
     * @return results keyed by application code
     * @throws DefinitionSyncException if any application's any category
     *         came back {@link Category.Failed} — {@link
     *         DefinitionSyncException#resultsByApplication()} carries every
     *         application's result, including the ones that synced fully
     */
    public Map<String, SyncResult> syncGrouped(List<DefinitionSet> sets, SyncOptions options) {
        Map<String, List<DefinitionSet>> byApp = new LinkedHashMap<>();
        for (DefinitionSet set : sets) {
            byApp.computeIfAbsent(set.applicationCode(), k -> new ArrayList<>()).add(set);
        }

        Map<String, SyncResult> results = new LinkedHashMap<>();
        for (Map.Entry<String, List<DefinitionSet>> entry : byApp.entrySet()) {
            results.put(entry.getKey(), syncMerged(entry.getKey(), entry.getValue(), options));
        }
        return DefinitionSyncException.throwIfAnyFailed(results);
    }

    /**
     * The actual merge: pool every contributing set's rows per category
     * (stamping connections/subscriptions with their effective client along
     * the way) and sync each category's combined list exactly once — reusing
     * the exact same per-category helpers {@link #sync} calls, so a single
     * merged set behaves identically to an equivalent hand-built one.
     */
    private SyncResult syncMerged(String app, List<DefinitionSet> sets, SyncOptions options) {
        boolean removeUnlisted = options.removeUnlisted();

        List<Definitions.Role> roles = new ArrayList<>();
        List<Definitions.EventType> eventTypes = new ArrayList<>();
        List<Definitions.DispatchPool> dispatchPools = new ArrayList<>();
        List<Definitions.Principal> principals = new ArrayList<>();
        List<Definitions.Process> processes = new ArrayList<>();
        // Scheduled jobs are concatenated as-is — clientId on a job is a
        // deliberately independent, explicit-only axis (see
        // Definitions.ScheduledJob) that merging must not default from a
        // set's own client.
        List<Definitions.ScheduledJob> scheduledJobs = new ArrayList<>();
        List<ConnectionRow> connectionRows = new ArrayList<>();
        List<SubscriptionRow> subscriptionRows = new ArrayList<>();
        Map<String, Object> openapiSpec = null;

        for (DefinitionSet set : sets) {
            roles.addAll(set.roles());
            eventTypes.addAll(set.eventTypes());
            dispatchPools.addAll(set.dispatchPools());
            principals.addAll(set.principals());
            processes.addAll(set.processes());
            scheduledJobs.addAll(set.scheduledJobs());
            connectionRows.addAll(toConnectionRows(set));
            subscriptionRows.addAll(toSubscriptionRows(set));
            // Only one OpenAPI document makes sense per application; keep
            // whichever set actually attached one (first wins — arbitrary
            // but deterministic).
            if (openapiSpec == null) {
                openapiSpec = set.openapiSpec();
            }
        }

        Category rolesResult = options.skips(SyncCategory.ROLES) || roles.isEmpty()
                ? Category.SKIPPED
                : syncRoles(app, roles, removeUnlisted);
        Category eventTypesResult = options.skips(SyncCategory.EVENT_TYPES) || eventTypes.isEmpty()
                ? Category.SKIPPED
                : syncEventTypes(app, eventTypes, removeUnlisted);

        ScopedResult scoped = syncConnectionsAndSubscriptions(
                app, connectionRows, subscriptionRows, options, subscriptionTargetBaseUrl);

        Category dispatchPoolsResult =
                options.skips(SyncCategory.DISPATCH_POOLS) || dispatchPools.isEmpty()
                        ? Category.SKIPPED
                        : syncDispatchPools(app, dispatchPools, removeUnlisted);
        Category principalsResult = options.skips(SyncCategory.PRINCIPALS) || principals.isEmpty()
                ? Category.SKIPPED
                : post(app, "principals", Map.of("principals", principals), removeUnlisted);
        Category processesResult = options.skips(SyncCategory.PROCESSES) || processes.isEmpty()
                ? Category.SKIPPED
                : post(app, "processes", Map.of("processes", processes), removeUnlisted);
        Category scheduledJobsResult =
                options.skips(SyncCategory.SCHEDULED_JOBS) || scheduledJobs.isEmpty()
                        ? Category.SKIPPED
                        : syncScheduledJobs(app, scheduledJobs, removeUnlisted);
        Category openapiResult = options.skips(SyncCategory.OPENAPI) || openapiSpec == null
                ? Category.SKIPPED
                : syncOpenapi(app, openapiSpec);

        return new SyncResult(app, rolesResult, eventTypesResult, scoped.subscriptions(),
                dispatchPoolsResult, principalsResult, processesResult, scheduledJobsResult,
                openapiResult, scoped.connections());
    }

    // ── connections + subscriptions: client grouping ───────────────────

    /** One connection row plus its resolved effective client (routing only, never posted). */
    private record ConnectionRow(Definitions.Connection connection, String client) {}

    /**
     * One subscription row plus its resolved effective client and the base
     * URL its path-style target should resolve against — carried
     * separately from the public {@link Definitions.Subscription} record so
     * a merged pool of rows from several sets (each with their own {@link
     * DefinitionSet#targetBaseUrl}) can each resolve correctly; this base
     * URL is never a wire field.
     */
    private record SubscriptionRow(Definitions.Subscription subscription, String client, String targetBaseUrl) {}

    private record ScopedResult(Category connections, Category subscriptions) {}

    private static List<ConnectionRow> toConnectionRows(DefinitionSet set) {
        List<ConnectionRow> rows = new ArrayList<>(set.connections().size());
        for (Definitions.Connection connection : set.connections()) {
            rows.add(new ConnectionRow(connection, effectiveClient(connection.client(), set.client())));
        }
        return rows;
    }

    private static List<SubscriptionRow> toSubscriptionRows(DefinitionSet set) {
        List<SubscriptionRow> rows = new ArrayList<>(set.subscriptions().size());
        for (Definitions.Subscription subscription : set.subscriptions()) {
            rows.add(new SubscriptionRow(
                    subscription, effectiveClient(subscription.client(), set.client()),
                    set.targetBaseUrl()));
        }
        return rows;
    }

    /** A row's own client wins; otherwise its owning set's client applies (null = global). */
    private static String effectiveClient(String rowClient, String setClient) {
        return (rowClient != null && !rowClient.isBlank()) ? rowClient : setClient;
    }

    /**
     * Sync connections, then subscriptions — grouped by resolved client (one
     * platform call per distinct client per resource).
     *
     * <p>Ordering, per the platform's ownership model:
     * <ul>
     *   <li>the global group (no client) is processed before any client
     *       group, because a client-scoped subscription may reference a
     *       global connection;
     *   <li>within EACH group, connections are synced before subscriptions,
     *       because a subscription's {@code connectionCode} must resolve in
     *       the same run;
     *   <li>if a group's connection sync fails, that group's subscriptions
     *       are skipped entirely (their connection codes may not resolve) —
     *       recorded as an error rather than sent as a request that would
     *       404.
     * </ul>
     */
    private ScopedResult syncConnectionsAndSubscriptions(
            String app,
            List<ConnectionRow> connectionRows,
            List<SubscriptionRow> subscriptionRows,
            SyncOptions options,
            String defaultTargetBaseUrl) {

        boolean doConnections = !options.skips(SyncCategory.CONNECTIONS) && !connectionRows.isEmpty();
        boolean doSubscriptions =
                !options.skips(SyncCategory.SUBSCRIPTIONS) && !subscriptionRows.isEmpty();
        if (!doConnections && !doSubscriptions) {
            return new ScopedResult(Category.SKIPPED, Category.SKIPPED);
        }

        Map<String, List<ConnectionRow>> connectionGroups =
                doConnections ? groupByClient(connectionRows, ConnectionRow::client) : Map.of();
        Map<String, List<SubscriptionRow>> subscriptionGroups =
                doSubscriptions ? groupByClient(subscriptionRows, SubscriptionRow::client) : Map.of();

        LinkedHashSet<String> keys = new LinkedHashSet<>();
        keys.addAll(connectionGroups.keySet());
        keys.addAll(subscriptionGroups.keySet());
        List<String> orderedKeys = new ArrayList<>(keys);
        // Stable sort: the global group ('') moves to the front, client
        // groups keep their relative (first-seen) order after it.
        orderedKeys.sort(Comparator.comparing(k -> !k.isEmpty()));

        int connCreated = 0;
        int connUpdated = 0;
        int connDeleted = 0;
        List<String> connCodes = new ArrayList<>();
        List<String> connErrors = new ArrayList<>();
        int subCreated = 0;
        int subUpdated = 0;
        int subDeleted = 0;
        List<String> subCodes = new ArrayList<>();
        List<String> subErrors = new ArrayList<>();
        Set<String> failedScopes = new HashSet<>();

        for (String key : orderedKeys) {
            String clientId = key.isEmpty() ? null : key;

            if (connectionGroups.containsKey(key)) {
                Category result = syncConnectionGroup(
                        app, connectionGroups.get(key), clientId, options.removeUnlisted());
                if (result instanceof Category.Synced s) {
                    connCreated += s.created();
                    connUpdated += s.updated();
                    connDeleted += s.deleted();
                    connCodes.addAll(s.syncedCodes());
                } else if (result instanceof Category.Failed f) {
                    connCreated += f.created();
                    connUpdated += f.updated();
                    connDeleted += f.deleted();
                    connCodes.addAll(f.syncedCodes());
                    connErrors.add(f.error());
                    failedScopes.add(key);
                }
            }

            if (!subscriptionGroups.containsKey(key)) {
                continue;
            }

            if (failedScopes.contains(key)) {
                subErrors.add("Skipped subscription sync for "
                        + (key.isEmpty() ? "the global scope" : "client \"" + key + "\"")
                        + ": its connection sync failed first");
                continue;
            }

            Category result = syncSubscriptionGroup(
                    app, subscriptionGroups.get(key), clientId, options.removeUnlisted(),
                    defaultTargetBaseUrl);
            if (result instanceof Category.Synced s) {
                subCreated += s.created();
                subUpdated += s.updated();
                subDeleted += s.deleted();
                subCodes.addAll(s.syncedCodes());
            } else if (result instanceof Category.Failed f) {
                subCreated += f.created();
                subUpdated += f.updated();
                subDeleted += f.deleted();
                subCodes.addAll(f.syncedCodes());
                subErrors.add(f.error());
            }
        }

        Category connectionsResult = !doConnections
                ? Category.SKIPPED
                : connErrors.isEmpty()
                        ? new Category.Synced(app, connCreated, connUpdated, connDeleted, connCodes)
                        : new Category.Failed(
                                connCreated, connUpdated, connDeleted, connCodes,
                                String.join("; ", connErrors));
        Category subscriptionsResult = !doSubscriptions
                ? Category.SKIPPED
                : subErrors.isEmpty()
                        ? new Category.Synced(app, subCreated, subUpdated, subDeleted, subCodes)
                        : new Category.Failed(
                                subCreated, subUpdated, subDeleted, subCodes,
                                String.join("; ", subErrors));

        return new ScopedResult(connectionsResult, subscriptionsResult);
    }

    /** Sync one client-group's worth of connections. */
    private Category syncConnectionGroup(
            String app, List<ConnectionRow> rows, String clientId, boolean removeUnlisted) {
        // Two sets contributing to the SAME (application, client) scope
        // (most commonly after syncGrouped() merges them) defining the same
        // connection code is a configuration error — fail locally, naming
        // the code and scope, rather than sending a request the platform
        // will reject or silently keeping whichever row happened to be last.
        List<String> duplicates = findDuplicates(rows.stream().map(r -> r.connection().code()).toList());
        if (!duplicates.isEmpty()) {
            return new Category.Failed(0, 0, 0, List.of(), String.format(
                    "Duplicate connection code(s) for %s: %s", scopeLabel(app, clientId),
                    String.join(", ", duplicates)));
        }

        try {
            List<Definitions.Connection> wire = rows.stream().map(ConnectionRow::connection).toList();
            return postScoped(app, "connections", clientId, "connections", wire, removeUnlisted);
        } catch (FlowCatalystException e) {
            return new Category.Failed(0, 0, 0, List.of(), e.getMessage());
        }
    }

    /** Sync one client-group's worth of subscriptions. */
    private Category syncSubscriptionGroup(
            String app, List<SubscriptionRow> rows, String clientId, boolean removeUnlisted,
            String defaultTargetBaseUrl) {
        // Two sets contributing to the SAME (application, client) scope
        // defining the same subscription code is a configuration error —
        // fail locally rather than sending a request the platform will
        // reject or silently keeping whichever row happened to be last.
        List<String> duplicates =
                findDuplicates(rows.stream().map(r -> r.subscription().code()).toList());
        if (!duplicates.isEmpty()) {
            return new Category.Failed(0, 0, 0, List.of(), String.format(
                    "Duplicate subscription code(s) for %s: %s", scopeLabel(app, clientId),
                    String.join(", ", duplicates)));
        }

        // Resolve every target BEFORE building the payload, and refuse the
        // whole group if any is missing. Sending only the resolvable ones is
        // not an option: with removeUnlisted the omitted subscriptions would
        // be DELETED.
        List<String> unresolved = new ArrayList<>();
        List<Map<String, Object>> wire = new ArrayList<>();
        for (SubscriptionRow row : rows) {
            String baseUrl = row.targetBaseUrl() != null ? row.targetBaseUrl() : defaultTargetBaseUrl;
            String resolved = resolveSubscriptionTarget(row.subscription().target(), baseUrl);
            if (resolved == null) {
                unresolved.add(row.subscription().code());
                continue;
            }
            @SuppressWarnings("unchecked")
            Map<String, Object> asMap =
                    transport.mapper().convertValue(row.subscription(), Map.class);
            asMap.put("target", resolved);
            wire.add(asMap);
        }
        if (!unresolved.isEmpty()) {
            return new Category.Failed(0, 0, 0, List.of(),
                    "No delivery target for subscription(s): " + String.join(", ", unresolved)
                            + ". `target` must be an absolute URL, or a path — which needs a"
                            + " synchronizer-level subscriptionTargetBaseUrl, or the set's"
                            + " forClient() base, to resolve against.");
        }

        try {
            return postScoped(app, "subscriptions", clientId, "subscriptions", wire, removeUnlisted);
        } catch (FlowCatalystException e) {
            return new Category.Failed(0, 0, 0, List.of(), e.getMessage());
        }
    }

    /**
     * The absolute delivery URL for one subscription row, or null when it
     * cannot be determined. An absolute target (anything with a scheme) is
     * used verbatim; a path is joined onto the base URL.
     */
    private static String resolveSubscriptionTarget(String rawTarget, String baseUrl) {
        String target = rawTarget == null ? "" : rawTarget.trim();
        if (target.isEmpty()) {
            return null;
        }
        if (HAS_SCHEME.matcher(target).find()) {
            return target;
        }
        if (baseUrl == null || baseUrl.isBlank()) {
            return null;
        }
        String base = baseUrl.trim().replaceAll("/+$", "");
        String path = target.replaceFirst("^/+", "");
        return base + "/" + path;
    }

    private static <T> Map<String, List<T>> groupByClient(List<T> rows, Function<T, String> clientOf) {
        Map<String, List<T>> groups = new LinkedHashMap<>();
        for (T row : rows) {
            String client = clientOf.apply(row);
            groups.computeIfAbsent(client == null ? "" : client, k -> new ArrayList<>()).add(row);
        }
        return groups;
    }

    /** {@code POST /api/applications/{app}/{resource}/sync?removeUnlisted=} with {@code clientId} in the body. */
    private Category.Synced postScoped(
            String app, String resource, String clientId, String wireKey, List<?> entries,
            boolean removeUnlisted) {
        Map<String, Object> body = new LinkedHashMap<>();
        if (clientId != null) {
            body.put("clientId", clientId);
        }
        body.put(wireKey, entries);
        return transport.post(
                "/api/applications/" + Transport.enc(app) + "/" + resource + "/sync",
                Map.of("removeUnlisted", removeUnlisted),
                body,
                Category.Synced.class);
    }

    // ── duplicate-code validation ────────────────────────────────────

    /**
     * Values appearing more than once in {@code values} — a configuration
     * error when it happens (two definitions colliding in the same sync
     * scope), most commonly surfacing after {@link #syncGrouped} merges two
     * otherwise individually-valid sets for the same application (or
     * application + client) into one call. Blank values are ignored —
     * already invalid on their own terms, reported elsewhere.
     */
    private static List<String> findDuplicates(List<String> values) {
        Map<String, Integer> counts = new LinkedHashMap<>();
        for (String value : values) {
            if (value == null || value.isBlank()) {
                continue;
            }
            counts.merge(value, 1, Integer::sum);
        }
        List<String> duplicates = new ArrayList<>();
        counts.forEach((value, count) -> {
            if (count > 1) {
                duplicates.add(value);
            }
        });
        return duplicates;
    }

    /** Human-readable label for a sync scope, used in duplicate-code error messages. */
    private static String scopeLabel(String app, String clientId) {
        return clientId == null
                ? "application \"" + app + "\""
                : "application \"" + app + "\", client \"" + clientId + "\"";
    }

    // ── per-category callers ────────────────────────────────────────

    private Category syncRoles(String app, List<Definitions.Role> roles, boolean removeUnlisted) {
        List<String> duplicates = findDuplicates(roles.stream().map(Definitions.Role::name).toList());
        if (!duplicates.isEmpty()) {
            return new Category.Failed(0, 0, 0, List.of(), String.format(
                    "Duplicate role name(s) for %s: %s", scopeLabel(app, null),
                    String.join(", ", duplicates)));
        }

        // Resolve permission refs to full strings so the wire shape is
        // {name, displayName?, description?, permissions: [string], clientManaged?}.
        List<Map<String, Object>> wire = roles.stream().map(role -> {
            Map<String, Object> entry = new LinkedHashMap<String, Object>();
            entry.put("name", role.name());
            putIfNotNull(entry, "displayName", role.displayName());
            putIfNotNull(entry, "description", role.description());
            if (role.permissions() != null) {
                entry.put("permissions",
                        role.permissions().stream().map(p -> p.resolve(app)).toList());
            }
            putIfNotNull(entry, "clientManaged", role.clientManaged());
            return entry;
        }).toList();
        return post(app, "roles", Map.of("roles", wire), removeUnlisted);
    }

    private Category syncEventTypes(
            String app, List<Definitions.EventType> eventTypes, boolean removeUnlisted) {
        List<String> duplicates =
                findDuplicates(eventTypes.stream().map(Definitions.EventType::code).toList());
        if (!duplicates.isEmpty()) {
            return new Category.Failed(0, 0, 0, List.of(), String.format(
                    "Duplicate event type code(s) for %s: %s", scopeLabel(app, null),
                    String.join(", ", duplicates)));
        }
        return post(app, "event-types", Map.of("eventTypes", eventTypes), removeUnlisted);
    }

    private Category syncDispatchPools(
            String app, List<Definitions.DispatchPool> pools, boolean removeUnlisted) {
        List<String> duplicates =
                findDuplicates(pools.stream().map(Definitions.DispatchPool::code).toList());
        if (!duplicates.isEmpty()) {
            return new Category.Failed(0, 0, 0, List.of(), String.format(
                    "Duplicate dispatch pool code(s) for %s: %s", scopeLabel(app, null),
                    String.join(", ", duplicates)));
        }
        return post(app, "dispatch-pools", Map.of("pools", pools), removeUnlisted);
    }

    /** Wire shape of the scheduled-jobs sync response. */
    private record ScheduledJobsWire(
            String applicationCode, List<String> created, List<String> updated,
            List<String> archived) {}

    private Category syncScheduledJobs(
            String app, List<Definitions.ScheduledJob> jobs, boolean removeUnlisted) {
        // Scheduled-jobs sync is the one endpoint that uses `archiveUnlisted`
        // in the body rather than `removeUnlisted` as a query param, and takes
        // one `clientId` per call rather than per job: group jobs by clientId
        // and issue one request per distinct group — `clientId` must NOT ride
        // along inside each job object (the API rejects unknown fields).
        Map<String, List<Definitions.ScheduledJob>> groups = new LinkedHashMap<>();
        for (Definitions.ScheduledJob job : jobs) {
            groups.computeIfAbsent(job.clientId() == null ? "" : job.clientId(),
                    k -> new ArrayList<>()).add(job);
        }

        int created = 0;
        int updated = 0;
        int deleted = 0;
        List<String> syncedCodes = new ArrayList<>();
        List<String> errors = new ArrayList<>();
        for (Map.Entry<String, List<Definitions.ScheduledJob>> group : groups.entrySet()) {
            String clientId = group.getKey().isEmpty() ? null : group.getKey();

            // Two sets contributing jobs to the SAME (application, clientId)
            // scope (most commonly after syncGrouped() merges them) defining
            // the same job code is a configuration error — fail locally
            // rather than sending a request the platform will reject.
            List<String> duplicates = findDuplicates(
                    group.getValue().stream().map(Definitions.ScheduledJob::code).toList());
            if (!duplicates.isEmpty()) {
                errors.add(String.format("Duplicate scheduled job code(s) for %s: %s",
                        scopeLabel(app, clientId), String.join(", ", duplicates)));
                continue;
            }

            List<Map<String, Object>> wireJobs = group.getValue().stream().map(job -> {
                @SuppressWarnings("unchecked")
                Map<String, Object> asMap =
                        transport.mapper().convertValue(job, Map.class);
                asMap.remove("clientId");
                return asMap;
            }).toList();

            Map<String, Object> body = new LinkedHashMap<>();
            if (!group.getKey().isEmpty()) {
                body.put("clientId", group.getKey());
            }
            body.put("jobs", wireJobs);
            body.put("archiveUnlisted", removeUnlisted);

            ScheduledJobsWire result = transport.post(
                    "/api/applications/" + Transport.enc(app) + "/scheduled-jobs/sync",
                    body,
                    ScheduledJobsWire.class);
            created += result.created().size();
            updated += result.updated().size();
            deleted += result.archived().size();
            syncedCodes.addAll(result.created());
            syncedCodes.addAll(result.updated());
        }

        if (!errors.isEmpty()) {
            return new Category.Failed(created, updated, deleted, syncedCodes, String.join("; ", errors));
        }
        return new Category.Synced(app, created, updated, deleted, syncedCodes);
    }

    /** Wire shape of the OpenAPI sync response. */
    private record OpenapiWire(
            String applicationCode, String version, String archivedPriorVersion, Boolean unchanged) {}

    private Category syncOpenapi(String app, Map<String, Object> spec) {
        // OpenAPI sync is one-shot — body is {spec}, not a list; normalise the
        // response to the per-category shape so callers can iterate uniformly.
        OpenapiWire result = transport.post(
                "/api/applications/" + Transport.enc(app) + "/openapi/sync",
                Map.of("spec", spec),
                OpenapiWire.class);
        boolean unchanged = Boolean.TRUE.equals(result.unchanged());
        int created = unchanged || result.archivedPriorVersion() != null ? 0 : 1;
        int updated = result.archivedPriorVersion() != null ? 1 : 0;
        return new Category.Synced(result.applicationCode(), created, updated, 0,
                List.of(result.version()));
    }

    // ── transport ───────────────────────────────────────────────────

    private Category post(String app, String resource, Object body, boolean removeUnlisted) {
        return transport.post(
                "/api/applications/" + Transport.enc(app) + "/" + resource + "/sync",
                Map.of("removeUnlisted", removeUnlisted),
                body,
                Category.Synced.class);
    }

    private static void putIfNotNull(Map<String, Object> map, String key, Object value) {
        if (value != null) map.put(key, value);
    }
}
