package io.flowcatalyst.sdk.sync;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import io.flowcatalyst.sdk.FlowCatalystClient;
import io.flowcatalyst.sdk.StubServer;
import io.flowcatalyst.sdk.sync.Definitions.Connection;
import io.flowcatalyst.sdk.sync.Definitions.DefinitionSet;
import io.flowcatalyst.sdk.sync.Definitions.Subscription;
import io.flowcatalyst.sdk.sync.Definitions.SubscriptionEventType;
import io.flowcatalyst.sdk.sync.SyncResult.Category;
import java.util.List;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

/**
 * Single-set connection/subscription sync: ordering, per-row client
 * grouping, and target resolution. Cross-set merging is covered separately
 * in {@link MergedSyncTest}.
 */
class ConnectionSyncTest {

    private static final ObjectMapper MAPPER = new ObjectMapper();
    private static final String SYNC_OK =
            "{\"applicationCode\":\"orders\",\"created\":1,\"updated\":0,\"deleted\":0,"
                    + "\"syncedCodes\":[\"x\"]}";

    private StubServer server;

    @BeforeEach
    void setUp() throws Exception {
        server = new StubServer();
        server.stubToken("tok");
    }

    @AfterEach
    void tearDown() {
        server.close();
    }

    private FlowCatalystClient client() {
        return FlowCatalystClient.builder()
                .baseUrl(server.baseUrl())
                .clientCredentials("id", "secret")
                .build();
    }

    private List<StubServer.Recorded> syncCalls() {
        return server.requests.stream().filter(r -> r.pathAndQuery().contains("/sync")).toList();
    }

    @Test
    void connectionsSyncBeforeSubscriptions() {
        server.on("POST", "/api/applications/orders/connections/sync", 200, SYNC_OK);
        server.on("POST", "/api/applications/orders/subscriptions/sync", 200, SYNC_OK);

        DefinitionSet set = DefinitionSet.define("orders")
                .withConnections(List.of(Connection.of("conn-a", "A")))
                .withSubscriptions(List.of(Subscription.of(
                        "sub-a", "Sub A", "https://a.example.com/hook",
                        List.of(SubscriptionEventType.of("orders:sales:order:created")))
                        .withConnectionCode("conn-a")));

        client().definitions().sync(set, SyncOptions.removingUnlisted());

        List<String> paths = syncCalls().stream().map(StubServer.Recorded::pathAndQuery).toList();
        assertEquals(2, paths.size());
        assertTrue(paths.get(0).startsWith("/api/applications/orders/connections/sync"),
                "connections first, was: " + paths);
        assertTrue(paths.get(1).startsWith("/api/applications/orders/subscriptions/sync"),
                "subscriptions second, was: " + paths);
    }

    /**
     * Entries for client A, client B and no client, all in ONE set (via
     * per-row {@code withClient}) → three connection requests + three
     * subscription requests, the right {@code clientId} each (absent for
     * the global one), global first — never merged into fewer calls.
     */
    @Test
    void rowLevelClientSplitsIntoOneCallPerClientGlobalFirst() throws Exception {
        server.on("POST", "/api/applications/orders/connections/sync", 200, SYNC_OK);
        server.on("POST", "/api/applications/orders/subscriptions/sync", 200, SYNC_OK);

        DefinitionSet set = DefinitionSet.define("orders")
                .withConnections(List.of(
                        Connection.of("conn-b", "B").withClient("beta"),
                        Connection.of("conn-none", "None"),
                        Connection.of("conn-a", "A").withClient("acme")))
                .withSubscriptions(List.of(
                        Subscription.of("sub-b", "Sub B", "https://x/hook",
                                        List.of(SubscriptionEventType.of("orders:sales:order:created")))
                                .withConnectionCode("conn-b").withClient("beta"),
                        Subscription.of("sub-none", "Sub None", "https://x/hook",
                                        List.of(SubscriptionEventType.of("orders:sales:order:created")))
                                .withConnectionCode("conn-none"),
                        Subscription.of("sub-a", "Sub A", "https://x/hook",
                                        List.of(SubscriptionEventType.of("orders:sales:order:created")))
                                .withConnectionCode("conn-a").withClient("acme")));

        client().definitions().sync(set, SyncOptions.removingUnlisted());

        List<StubServer.Recorded> connCalls = server.requests.stream()
                .filter(r -> r.pathAndQuery().contains("connections/sync")).toList();
        List<StubServer.Recorded> subCalls = server.requests.stream()
                .filter(r -> r.pathAndQuery().contains("subscriptions/sync")).toList();
        assertEquals(3, connCalls.size());
        assertEquals(3, subCalls.size());

        JsonNode connGlobal = MAPPER.readTree(connCalls.get(0).body());
        assertFalse(connGlobal.has("clientId"), "global first, no clientId key");
        assertEquals(1, connGlobal.get("connections").size());

        JsonNode subGlobal = MAPPER.readTree(subCalls.get(0).body());
        assertFalse(subGlobal.has("clientId"));

        List<String> connClientIds = connCalls.stream()
                .skip(1)
                .map(c -> {
                    try {
                        JsonNode n = MAPPER.readTree(c.body()).get("clientId");
                        return n == null ? null : n.asText();
                    } catch (Exception e) {
                        throw new RuntimeException(e);
                    }
                })
                .toList();
        assertTrue(connClientIds.containsAll(List.of("beta", "acme")), "both client scopes present");
    }

    @Test
    void absoluteTargetSentVerbatim() throws Exception {
        server.on("POST", "/api/applications/orders/subscriptions/sync", 200, SYNC_OK);

        DefinitionSet set = DefinitionSet.define("orders")
                .withSubscriptions(List.of(Subscription.of(
                        "sub-a", "Sub A", "https://absolute.example.com/hook",
                        List.of(SubscriptionEventType.of("orders:sales:order:created")))));

        client().definitions().sync(set);

        JsonNode body = MAPPER.readTree(syncCalls().get(0).body());
        assertEquals("https://absolute.example.com/hook",
                body.get("subscriptions").get(0).get("target").asText());
    }

    @Test
    void pathTargetWithNoBaseAvailableFailsLocallyNamingTheSubscriptionAndThrows() {
        DefinitionSet set = DefinitionSet.define("orders")
                .withSubscriptions(List.of(Subscription.of(
                        "sub-a", "Sub A", "/webhooks/orders",
                        List.of(SubscriptionEventType.of("orders:sales:order:created")))));

        DefinitionSyncException ex = assertThrows(
                DefinitionSyncException.class, () -> client().definitions().sync(set));

        Category.Failed failed = assertInstanceOf(Category.Failed.class, ex.result().subscriptions());
        assertTrue(failed.error().contains("sub-a"), "names the subscription: " + failed.error());
        assertTrue(ex.getMessage().contains("sub-a"), "exception message names it too: " + ex.getMessage());
        assertTrue(syncCalls().isEmpty(), "no request sent — a partial list would delete under removeUnlisted");
    }

    @Test
    void pathTargetResolvesAgainstSynchronizerLevelDefault() throws Exception {
        server.on("POST", "/api/applications/orders/subscriptions/sync", 200, SYNC_OK);

        var synchronizer = new DefinitionSynchronizer(client().transport(), "https://default.example.com");
        DefinitionSet set = DefinitionSet.define("orders")
                .withSubscriptions(List.of(Subscription.of(
                        "sub-a", "Sub A", "/webhooks/orders",
                        List.of(SubscriptionEventType.of("orders:sales:order:created")))));

        synchronizer.sync(set);

        JsonNode body = MAPPER.readTree(syncCalls().get(0).body());
        assertEquals("https://default.example.com/webhooks/orders",
                body.get("subscriptions").get(0).get("target").asText());
    }

    @Test
    void duplicateSubscriptionCodeWithinOneSetFailsLocallyAndThrows() {
        DefinitionSet set = DefinitionSet.define("orders")
                .withSubscriptions(List.of(
                        Subscription.of("dup", "First", "https://a.example.com/hook",
                                List.of(SubscriptionEventType.of("orders:sales:order:created"))),
                        Subscription.of("dup", "Second", "https://b.example.com/hook",
                                List.of(SubscriptionEventType.of("orders:sales:order:created")))));

        DefinitionSyncException ex = assertThrows(
                DefinitionSyncException.class, () -> client().definitions().sync(set));

        Category.Failed failed = assertInstanceOf(Category.Failed.class, ex.result().subscriptions());
        assertTrue(failed.error().contains("dup"));
        assertTrue(syncCalls().isEmpty());
    }
}
