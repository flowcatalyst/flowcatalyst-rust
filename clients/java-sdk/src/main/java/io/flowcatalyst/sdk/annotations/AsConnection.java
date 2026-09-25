package io.flowcatalyst.sdk.annotations;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

/**
 * Declares a connection this application owns. Place on any class and
 * register it with {@link DefinitionScanner}.
 *
 * <p>A connection is application-owned: the platform assigns its service
 * account itself (the application's own provisioned one), so this annotation
 * carries nothing environment-specific — no service account id, no secret.
 * It exists purely to give a subscription's {@code connectionCode} something
 * to resolve, and connections are synced BEFORE subscriptions for that
 * reason.
 *
 * <pre>{@code
 * @AsConnection(
 *         code = "orders-webhook",
 *         name = "Orders Webhook",
 *         description = "Delivers order events to the orders service")
 * final class OrdersWebhookConnection {}
 * }</pre>
 */
@Retention(RetentionPolicy.RUNTIME)
@Target(ElementType.TYPE)
public @interface AsConnection {

    /**
     * Unique connection code (unique within the application) — stable
     * across environments, unlike its id. What a subscription's
     * {@code connectionCode} names.
     */
    String code();

    String name();

    String description() default "";

    /** Your own system's identifier for this connection, if any. */
    String externalId() default "";

    /**
     * The FlowCatalyst client (by identifier slug — never an id; ids differ
     * per environment) this connection is scoped to. Empty = the default
     * client passed to {@link DefinitionScanner#scan(String,
     * java.util.Collection, String)} (single-tenant apps), and empty there
     * too means global (no client). For a multi-tenant application, don't
     * set this on the annotation — build one {@link
     * io.flowcatalyst.sdk.sync.Definitions.DefinitionSet} per client instead
     * (see {@code DefinitionSet.forClient}).
     */
    String client() default "";
}
