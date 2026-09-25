package io.flowcatalyst.sdk.annotations;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

/**
 * Declares a dispatch pool this application expects. Register the annotated
 * class with {@link DefinitionScanner}.
 */
@Retention(RetentionPolicy.RUNTIME)
@Target(ElementType.TYPE)
public @interface AsDispatchPool {

    String code();

    String name() default "";

    String description() default "";

    /** Requests per minute; negative = platform default (100). */
    int rateLimit() default -1;

    /** Concurrency cap; negative = platform default (10). */
    int concurrency() default -1;
}
