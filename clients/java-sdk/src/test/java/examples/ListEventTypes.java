package examples;

import io.flowcatalyst.sdk.FlowCatalystClient;
import io.flowcatalyst.sdk.error.FlowCatalystException;
import io.flowcatalyst.sdk.generated.model.EventTypeListResponse;

/**
 * Minimal example: authenticate with client credentials and list event types.
 *
 * <pre>
 * export FC_BASE_URL=http://localhost:8080
 * export FC_CLIENT_ID=oac_...
 * export FC_CLIENT_SECRET=...
 * mvn -q test-compile org.codehaus.mojo:exec-maven-plugin:3.5.0:java \
 *     -Dexec.mainClass=examples.ListEventTypes -Dexec.classpathScope=test
 * </pre>
 */
public final class ListEventTypes {

    public static void main(String[] args) {
        FlowCatalystClient client = FlowCatalystClient.builder()
                .baseUrl(env("FC_BASE_URL", "http://localhost:8080"))
                .clientCredentials(env("FC_CLIENT_ID", null), env("FC_CLIENT_SECRET", null))
                .build();

        try {
            EventTypeListResponse response = client.eventTypes().list(null);
            System.out.println("Total event types: " + response.getItems().size());
            response.getItems().forEach(et ->
                    System.out.println("  " + et.getCode() + "  (" + et.getName() + ")"));
        } catch (FlowCatalystException e) {
            System.err.println("Failed [" + e.error().getClass().getSimpleName() + "]: "
                    + e.getMessage());
            System.exit(1);
        }
    }

    private static String env(String name, String fallback) {
        String value = System.getenv(name);
        if (value == null && fallback == null) {
            throw new IllegalStateException("Set " + name);
        }
        return value != null ? value : fallback;
    }
}
