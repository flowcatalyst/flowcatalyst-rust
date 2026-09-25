package io.flowcatalyst.sdk.sync;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

import io.flowcatalyst.sdk.sync.Definitions.DefinitionSet;
import org.junit.jupiter.api.Test;

/**
 * An application code can be given directly or inherited from the environment.
 * There is deliberately no per-definition override: the set a definition is
 * built into IS its application, and a codebase owning several applications
 * builds one set each and passes them to {@code definitions().syncAll(…)}.
 */
class ApplicationCodeTest {

    @Test
    void defineTakesTheApplicationCodeDirectly() {
        assertEquals("orders", DefinitionSet.define("orders").applicationCode());
    }

    @Test
    void defineFromEnvInheritsTheCodeFromTheEnvironment() {
        // The JDK offers no supported way to mutate the process environment,
        // so assert against whatever the surrounding environment provides:
        // set → inherited, unset → a clear failure. Both branches are real.
        String fromEnv = System.getenv(DefinitionSet.APP_CODE_ENV);

        if (fromEnv == null || fromEnv.isBlank()) {
            IllegalStateException e =
                    assertThrows(IllegalStateException.class, DefinitionSet::defineFromEnv);
            assertTrue(
                    e.getMessage().contains(DefinitionSet.APP_CODE_ENV),
                    "message should name the variable, was: " + e.getMessage());
        } else {
            assertEquals(fromEnv, DefinitionSet.defineFromEnv().applicationCode());
        }
    }

    /**
     * Guards the failure path explicitly when the variable is absent — a
     * missing code must not slip through and become a request to
     * {@code /api/applications/null/…}.
     */
    @Test
    void defineFromEnvThrowsWhenUnset() {
        assumeTrue(
                System.getenv(DefinitionSet.APP_CODE_ENV) == null,
                "only meaningful when " + DefinitionSet.APP_CODE_ENV + " is unset");

        assertThrows(IllegalStateException.class, DefinitionSet::defineFromEnv);
    }
}
