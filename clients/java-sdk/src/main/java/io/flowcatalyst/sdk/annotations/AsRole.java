package io.flowcatalyst.sdk.annotations;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

/**
 * Declares a role this application needs. Register the annotated class with
 * {@link DefinitionScanner}.
 *
 * <p>The role name must NOT include the {@code <app>:} prefix — the platform
 * adds it. Permissions are 4-part
 * {@code <application>:<context>:<aggregate>:<action>} strings; wildcards are
 * supported in any position.
 */
@Retention(RetentionPolicy.RUNTIME)
@Target(ElementType.TYPE)
public @interface AsRole {

    /** Short name (no {@code <app>:} prefix). */
    String name();

    String displayName() default "";

    String description() default "";

    /** Full 4-part permission strings. */
    String[] permissions() default {};

    /** When true, client admins can assign this role to their own users. */
    boolean clientManaged() default false;
}
