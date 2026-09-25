package io.flowcatalyst.sdk.sync;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import io.flowcatalyst.sdk.FlowCatalystClient;
import io.flowcatalyst.sdk.StubServer;
import io.flowcatalyst.sdk.annotations.AsDispatchPool;
import io.flowcatalyst.sdk.annotations.AsEventType;
import io.flowcatalyst.sdk.annotations.AsRole;
import io.flowcatalyst.sdk.annotations.AsSubscription;
import io.flowcatalyst.sdk.annotations.DefinitionScanner;
import io.flowcatalyst.sdk.sync.Definitions.DefinitionSet;
import io.flowcatalyst.sdk.sync.Definitions.Permission;
import io.flowcatalyst.sdk.sync.Definitions.PermissionRef;
import io.flowcatalyst.sdk.sync.SyncResult.Category;
import java.util.List;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

class DefinitionSyncTest {

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

    @Test
    void syncsCategoriesInOrderAndSkipsAbsentOnes() {
        server.on("POST", "/api/applications/orders/roles/sync", 200, SYNC_OK);
        server.on("POST", "/api/applications/orders/event-types/sync", 200, SYNC_OK);

        DefinitionSet set = DefinitionSet.define("orders")
                .withRoles(List.of(Definitions.Role.of("admin")))
                .withEventTypes(List.of(
                        Definitions.EventType.of("orders:sales:order:created", "Order Created")));

        SyncResult result = client().definitions().sync(set, SyncOptions.removingUnlisted());

        assertInstanceOf(Category.Synced.class, result.roles());
        assertInstanceOf(Category.Synced.class, result.eventTypes());
        assertEquals(Category.SKIPPED, result.subscriptions());
        assertEquals(Category.SKIPPED, result.openapi());

        List<String> syncCalls = server.requests.stream()
                .map(StubServer.Recorded::pathAndQuery)
                .filter(p -> p.contains("/sync"))
                .toList();
        assertEquals(2, syncCalls.size());
        assertTrue(syncCalls.get(0).startsWith("/api/applications/orders/roles/sync"), "roles first");
        assertTrue(syncCalls.stream().allMatch(p -> p.endsWith("removeUnlisted=true")));
    }

    @Test
    void rolePermissionsResolveAgainstTheApplicationCode() throws Exception {
        server.on("POST", "/api/applications/orders/roles/sync", 200, SYNC_OK);

        DefinitionSet set = DefinitionSet.define("orders")
                .withRoles(List.of(Definitions.Role.of("editor").withPermissions(List.of(
                        Permission.of("Posts", "Post", "Edit"),
                        PermissionRef.raw("ORDERS:admin:shipment:cancel")))));

        client().definitions().sync(set);

        JsonNode body = MAPPER.readTree(server.requests.getLast().body());
        JsonNode permissions = body.get("roles").get(0).get("permissions");
        assertEquals("orders:posts:post:edit", permissions.get(0).asText());
        assertEquals("orders:admin:shipment:cancel", permissions.get(1).asText());
    }

    @Test
    void scheduledJobsGroupByClientIdWithArchiveUnlistedInBody() throws Exception {
        server.on("POST", "/api/applications/orders/scheduled-jobs/sync", 200,
                "{\"applicationCode\":\"orders\",\"created\":[\"j1\"],\"updated\":[],"
                        + "\"archived\":[]}");

        DefinitionSet set = DefinitionSet.define("orders").withScheduledJobs(List.of(
                Definitions.ScheduledJob.of("job-a", "Job A", List.of("0 0 * * * *")),
                Definitions.ScheduledJob.of("job-b", "Job B", List.of("0 30 * * * *"))
                        .withClientId("clt_X")));

        SyncResult result = client().definitions().sync(set, SyncOptions.removingUnlisted());

        List<StubServer.Recorded> calls = server.requests.stream()
                .filter(r -> r.pathAndQuery().contains("scheduled-jobs/sync")).toList();
        assertEquals(2, calls.size(), "one request per clientId group");

        JsonNode platformBody = MAPPER.readTree(calls.get(0).body());
        assertFalse(platformBody.has("clientId"));
        assertTrue(platformBody.get("archiveUnlisted").asBoolean());
        assertFalse(platformBody.get("jobs").get(0).has("clientId"),
                "clientId must not ride along inside job objects");

        JsonNode clientBody = MAPPER.readTree(calls.get(1).body());
        assertEquals("clt_X", clientBody.get("clientId").asText());

        Category.Synced jobs = assertInstanceOf(Category.Synced.class, result.scheduledJobs());
        assertEquals(2, jobs.created(), "results merged across groups");
    }

    @Test
    void openapiResultIsNormalisedToCategoryShape() {
        server.on("POST", "/api/applications/orders/openapi/sync", 200,
                "{\"applicationCode\":\"orders\",\"version\":\"3\","
                        + "\"archivedPriorVersion\":\"2\",\"unchanged\":false}");

        SyncResult result = client().definitions().sync(
                DefinitionSet.define("orders")
                        .withOpenapiSpec(java.util.Map.of("openapi", "3.1.0")));

        Category.Synced openapi = assertInstanceOf(Category.Synced.class, result.openapi());
        assertEquals(0, openapi.created());
        assertEquals(1, openapi.updated());
        assertEquals(List.of("3"), openapi.syncedCodes());
    }

    @Test
    void skipFlagsForceSkipPresentCategories() {
        DefinitionSet set = DefinitionSet.define("orders")
                .withRoles(List.of(Definitions.Role.of("admin")));

        SyncResult result = client().definitions().sync(set,
                SyncOptions.defaults().skipping(SyncOptions.SyncCategory.ROLES));

        assertEquals(Category.SKIPPED, result.roles());
        assertTrue(server.requests.stream().noneMatch(r -> r.pathAndQuery().contains("/sync")));
    }

    // ── annotation scanning ─────────────────────────────────────────

    @AsEventType(code = "orders:sales:order:created", name = "Order Created",
            description = "A new order")
    record OrderCreated(String orderId) {}

    @AsSubscription(code = "order-hook", name = "Order Hook",
            target = "https://app.example.com/hook",
            eventTypes = {"orders:sales:order:created"},
            mode = "BLOCK_ON_ERROR", maxRetries = 7)
    static final class OrderHookHandler {}

    @AsDispatchPool(code = "orders-pool", rateLimit = 200)
    @AsRole(name = "admin", displayName = "Administrator",
            permissions = {"orders:admin:*:*"}, clientManaged = true)
    static final class OrdersDeclarations {}

    @Test
    void scannerBuildsDefinitionSetFromAnnotatedClasses() {
        DefinitionSet set = DefinitionScanner.scan("orders",
                List.of(OrderCreated.class, OrderHookHandler.class, OrdersDeclarations.class));

        assertEquals("orders", set.applicationCode());
        assertEquals("orders:sales:order:created", set.eventTypes().getFirst().code());
        assertEquals("A new order", set.eventTypes().getFirst().description());

        Definitions.Subscription subscription = set.subscriptions().getFirst();
        assertEquals(Definitions.SubscriptionMode.BLOCK_ON_ERROR, subscription.mode());
        assertEquals(7, subscription.maxRetries());
        assertEquals(null, subscription.timeoutSeconds(), "sentinel -1 becomes null");
        assertEquals("orders:sales:order:created",
                subscription.eventTypes().getFirst().eventTypeCode());

        Definitions.DispatchPool pool = set.dispatchPools().getFirst();
        assertEquals("orders-pool", pool.name(), "name defaults to code");
        assertEquals(200, pool.rateLimit());
        assertEquals(null, pool.concurrency());

        Definitions.Role role = set.roles().getFirst();
        assertEquals("admin", role.name());
        assertEquals(true, role.clientManaged());
        assertEquals("orders:admin:*:*", role.permissions().getFirst().resolve("orders"));
    }
}
