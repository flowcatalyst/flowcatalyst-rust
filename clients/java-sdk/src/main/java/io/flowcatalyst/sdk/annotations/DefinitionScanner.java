package io.flowcatalyst.sdk.annotations;

import io.flowcatalyst.sdk.sync.Definitions;
import io.flowcatalyst.sdk.sync.Definitions.DefinitionSet;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collection;
import java.util.List;

/**
 * Builds a {@link DefinitionSet} from explicitly registered annotated
 * classes. No classpath scanning — pass the classes carrying
 * {@link AsEventType}, {@link AsSubscription}, {@link AsDispatchPool}, and
 * {@link AsRole}:
 *
 * <pre>{@code
 * DefinitionSet set = DefinitionScanner.scan("orders",
 *         List.of(OrderCreated.class, OrderShippedHandler.class, AdminRole.class));
 * client.definitions().sync(set);
 * }</pre>
 */
public final class DefinitionScanner {

    private DefinitionScanner() {}

    public static DefinitionSet scan(String applicationCode, Collection<Class<?>> classes) {
        return scan(applicationCode, classes, null);
    }

    /**
     * As {@link #scan(String, Collection)}, additionally applying
     * {@code defaultClient} to any scanned {@link AsConnection} or
     * {@link AsSubscription} that doesn't set its own {@code client()}. This
     * is the single-tenant path — a definition's own {@code client()} always
     * wins. A multi-tenant application should NOT set a default client here;
     * build one {@link DefinitionSet} per (application, client) instead via
     * {@code DefinitionSet.forClient(...)}.
     *
     * @param defaultClient FlowCatalyst client (identifier slug); null or
     *        blank = global
     */
    public static DefinitionSet scan(
            String applicationCode, Collection<Class<?>> classes, String defaultClient) {
        List<Definitions.EventType> eventTypes = new ArrayList<>();
        List<Definitions.Subscription> subscriptions = new ArrayList<>();
        List<Definitions.Connection> connections = new ArrayList<>();
        List<Definitions.DispatchPool> pools = new ArrayList<>();
        List<Definitions.Role> roles = new ArrayList<>();
        String fallbackClient = emptyToNull(defaultClient);

        for (Class<?> clazz : classes) {
            AsEventType eventType = clazz.getAnnotation(AsEventType.class);
            if (eventType != null) {
                eventTypes.add(new Definitions.EventType(
                        eventType.code(), eventType.name(), emptyToNull(eventType.description())));
            }

            AsConnection connection = clazz.getAnnotation(AsConnection.class);
            if (connection != null) {
                connections.add(new Definitions.Connection(
                        connection.code(),
                        connection.name(),
                        emptyToNull(connection.description()),
                        emptyToNull(connection.externalId()),
                        resolveClient(connection.client(), fallbackClient)));
            }

            AsSubscription subscription = clazz.getAnnotation(AsSubscription.class);
            if (subscription != null) {
                subscriptions.add(new Definitions.Subscription(
                        subscription.code(),
                        subscription.name(),
                        emptyToNull(subscription.description()),
                        subscription.target(),
                        emptyToNull(subscription.connectionId()),
                        Arrays.stream(subscription.eventTypes())
                                .map(Definitions.SubscriptionEventType::of)
                                .toList(),
                        emptyToNull(subscription.dispatchPoolCode()),
                        subscription.mode().isEmpty()
                                ? null
                                : Definitions.SubscriptionMode.valueOf(subscription.mode()),
                        negativeToNull(subscription.maxRetries()),
                        negativeToNull(subscription.timeoutSeconds()),
                        subscription.dataOnly(),
                        emptyToNull(subscription.connectionCode()),
                        subscription.sharedConnection() ? Boolean.TRUE : null,
                        resolveClient(subscription.client(), fallbackClient)));
            }

            AsDispatchPool pool = clazz.getAnnotation(AsDispatchPool.class);
            if (pool != null) {
                pools.add(new Definitions.DispatchPool(
                        pool.code(),
                        pool.name().isEmpty() ? pool.code() : pool.name(),
                        emptyToNull(pool.description()),
                        negativeToNull(pool.rateLimit()),
                        negativeToNull(pool.concurrency())));
            }

            AsRole role = clazz.getAnnotation(AsRole.class);
            if (role != null) {
                roles.add(new Definitions.Role(
                        role.name(),
                        emptyToNull(role.displayName()),
                        emptyToNull(role.description()),
                        Arrays.stream(role.permissions())
                                .<Definitions.PermissionRef>map(Definitions.PermissionRef::raw)
                                .toList(),
                        role.clientManaged()));
            }
        }

        return DefinitionSet.define(applicationCode)
                .withEventTypes(eventTypes)
                .withConnections(connections)
                .withSubscriptions(subscriptions)
                .withDispatchPools(pools)
                .withRoles(roles);
    }

    /** The annotation's own client wins; falls back to the scanner's configured default. */
    private static String resolveClient(String annotationClient, String fallbackClient) {
        String own = emptyToNull(annotationClient);
        return own != null ? own : fallbackClient;
    }

    private static String emptyToNull(String value) {
        return value == null || value.isEmpty() ? null : value;
    }

    private static Integer negativeToNull(int value) {
        return value < 0 ? null : value;
    }
}
