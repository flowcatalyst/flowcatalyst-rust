package io.flowcatalyst.sdk.annotations;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

/**
 * Declares an event type published by this application. Place on the class
 * that represents the event and register the class with
 * {@link DefinitionScanner}.
 *
 * <pre>{@code
 * @AsEventType(code = "orders:sales:order:created", name = "Order Created")
 * public record OrderCreated(String orderId) {}
 * }</pre>
 */
@Retention(RetentionPolicy.RUNTIME)
@Target(ElementType.TYPE)
public @interface AsEventType {

    /** Full 4-part code: {@code <app>:<subdomain>:<aggregate>:<event>}. */
    String code();

    /** Human-readable label. */
    String name();

    String description() default "";
}
