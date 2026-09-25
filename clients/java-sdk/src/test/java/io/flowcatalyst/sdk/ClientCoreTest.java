package io.flowcatalyst.sdk;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.flowcatalyst.sdk.error.FlowCatalystException;
import io.flowcatalyst.sdk.error.SdkError;
import io.flowcatalyst.sdk.generated.model.EventTypeListResponse;
import java.time.Duration;
import java.util.List;
import java.util.Map;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

class ClientCoreTest {

    private StubServer server;

    @BeforeEach
    void setUp() throws Exception {
        server = new StubServer();
    }

    @AfterEach
    void tearDown() {
        server.close();
    }

    private FlowCatalystClient client() {
        return FlowCatalystClient.builder()
                .baseUrl(server.baseUrl())
                .clientCredentials("id", "secret")
                .retryDelay(Duration.ofMillis(1))
                .build();
    }

    @Test
    void clientCredentialsTokenIsFetchedCachedAndInjected() {
        server.stubToken("tok-1");
        server.on("GET", "/api/event-types", 200, "{\"eventTypes\":[],\"total\":0}");

        FlowCatalystClient client = client();
        EventTypeListResponse first = client.eventTypes().list(null);
        EventTypeListResponse second = client.eventTypes().list(null);

        assertNotNull(first);
        assertNotNull(second);
        long tokenFetches = server.requests.stream()
                .filter(r -> r.pathAndQuery().equals("/oauth/token")).count();
        assertEquals(1, tokenFetches, "token must be cached between calls");
        assertEquals("Bearer tok-1", server.requests.getLast().authorization());
    }

    @Test
    void unauthorizedTriggersOneTokenRefreshThenSucceeds() {
        AtomicInteger tokens = new AtomicInteger();
        server.on("POST", "/oauth/token", r -> StubServer.Reply.json(200,
                "{\"access_token\":\"tok-" + tokens.incrementAndGet()
                        + "\",\"token_type\":\"Bearer\",\"expires_in\":3600}"));
        server.on("GET", "/api/event-types", r ->
                "Bearer tok-2".equals(r.authorization())
                        ? StubServer.Reply.json(200, "{\"eventTypes\":[],\"total\":0}")
                        : StubServer.Reply.json(401, "{\"error\":\"UNAUTHORIZED\"}"));

        assertNotNull(client().eventTypes().list(null));
        assertEquals(2, tokens.get(), "401 should force exactly one refresh");
    }

    @Test
    void transientStatusesAreRetriedWithBackoff() {
        server.stubToken("tok");
        AtomicInteger calls = new AtomicInteger();
        server.on("GET", "/api/event-types", r ->
                calls.incrementAndGet() < 3
                        ? StubServer.Reply.json(503, "{\"error\":\"UNAVAILABLE\"}")
                        : StubServer.Reply.json(200, "{\"eventTypes\":[],\"total\":0}"));

        assertNotNull(client().eventTypes().list(null));
        assertEquals(3, calls.get());
    }

    @Test
    void retriesAreCappedAndSurfaceTheFinalError() {
        server.stubToken("tok");
        server.on("GET", "/api/event-types", 503, "{\"error\":\"UNAVAILABLE\"}");

        FlowCatalystException e = assertThrows(FlowCatalystException.class,
                () -> client().eventTypes().list(null));
        SdkError.HttpStatus status = assertInstanceOf(SdkError.HttpStatus.class, e.error());
        assertEquals(503, status.status());
        long apiCalls = server.requests.stream()
                .filter(r -> r.pathAndQuery().startsWith("/api/")).count();
        assertEquals(4, apiCalls, "initial call + 3 retries");
    }

    @Test
    void errorStatusesMapToTypedVariants() {
        server.stubToken("tok");
        server.on("GET", "/api/event-types/missing", 404,
                "{\"error\":\"NOT_FOUND\",\"message\":\"event type not found\"}");
        server.on("POST", "/api/event-types", 422,
                "{\"error\":\"VALIDATION\",\"message\":\"invalid\",\"errors\":{\"code\":[\"required\"]}}");
        server.on("POST", "/api/clients", 409,
                "{\"error\":\"DUPLICATE\",\"message\":\"identifier taken\"}");

        FlowCatalystClient client = client();

        FlowCatalystException notFound = assertThrows(FlowCatalystException.class,
                () -> client.eventTypes().get("missing"));
        assertEquals("event type not found",
                assertInstanceOf(SdkError.NotFound.class, notFound.error()).message());

        FlowCatalystException validation = assertThrows(FlowCatalystException.class,
                () -> client.eventTypes().create(
                        new io.flowcatalyst.sdk.generated.model.CreateEventTypeRequest()));
        SdkError.Validation v = assertInstanceOf(SdkError.Validation.class, validation.error());
        assertEquals(List.of("required"), v.errors().get("code"));

        FlowCatalystException conflict = assertThrows(FlowCatalystException.class,
                () -> client.clients().create(
                        new io.flowcatalyst.sdk.generated.model.CreateClientRequest()));
        assertEquals("DUPLICATE",
                assertInstanceOf(SdkError.Conflict.class, conflict.error()).code());
    }

    @Test
    void invalidCredentialsSurfaceAsTypedError() {
        server.on("POST", "/oauth/token", 401, "{\"error\":\"invalid_client\"}");

        FlowCatalystException e = assertThrows(FlowCatalystException.class,
                () -> client().eventTypes().list(null));
        assertInstanceOf(SdkError.InvalidCredentials.class, e.error());
    }

    @Test
    void userTokenModeDoesNotRefreshOn401() {
        server.on("GET", "/api/event-types", 401, "{\"error\":\"UNAUTHORIZED\"}");

        FlowCatalystClient client = FlowCatalystClient.builder()
                .baseUrl(server.baseUrl())
                .accessToken("user-token")
                .retryDelay(Duration.ofMillis(1))
                .build();

        FlowCatalystException e = assertThrows(FlowCatalystException.class,
                () -> client.eventTypes().list(null));
        assertInstanceOf(SdkError.TokenExpired.class, e.error());
        assertTrue(server.requests.stream().noneMatch(
                r -> r.pathAndQuery().equals("/oauth/token")), "must not call token endpoint");
        assertEquals("Bearer user-token", server.requests.getFirst().authorization());
    }

    @Test
    void queryParametersAreEncodedAndListsRepeated() {
        server.stubToken("tok");
        server.on("GET", "/api/principals", 200, "{\"principals\":[],\"total\":0}");

        client().principals().list(new io.flowcatalyst.sdk.resources.PrincipalsResource.Filters(
                "USER", null, true, "a b", List.of("r1", "r2"), 1, 20, null, null));

        String path = server.requests.getLast().pathAndQuery();
        assertTrue(path.contains("q=a+b"), path);
        assertTrue(path.contains("roles=r1&roles=r2"), path);
        assertTrue(path.contains("active=true"), path);
    }

    @Test
    void appScopedSyncHitsTheRightPathWithRemoveUnlisted() {
        server.stubToken("tok");
        server.on("POST", "/api/applications/my-app/event-types/sync", 200,
                "{\"created\":1,\"updated\":0,\"removed\":0,\"unchanged\":0}");

        client().eventTypes().sync("my-app",
                List.of(new io.flowcatalyst.sdk.generated.model.SyncEventTypeInputRequest()
                        .code("orders:order:created").name("Order Created")),
                true);

        StubServer.Recorded recorded = server.requests.getLast();
        assertTrue(recorded.pathAndQuery().endsWith("?removeUnlisted=true"), recorded.pathAndQuery());
        assertTrue(recorded.body().contains("orders:order:created"), recorded.body());
    }

    @Test
    void routerCallsAreUnauthenticatedAgainstRouterBaseUrl() throws Exception {
        try (StubServer routerServer = new StubServer()) {
            routerServer.on("POST", "/monitoring/in-flight-messages/check-batch", 200,
                    "{\"m1\":true,\"m2\":false}");

            FlowCatalystClient client = FlowCatalystClient.builder()
                    .baseUrl(server.baseUrl())
                    .clientCredentials("id", "secret")
                    .routerBaseUrl(routerServer.baseUrl())
                    .build();

            Map<String, Boolean> result = client.router().inPipelineBatch(List.of("m1", "m2"));
            assertEquals(Map.of("m1", true, "m2", false), result);
            assertEquals(null, routerServer.requests.getFirst().authorization());
        }
    }

    @Test
    void voidEndpointsAcceptEmptyBodies() {
        server.stubToken("tok");
        server.on("POST", "/api/subscriptions/sub_1/pause", 204, null);

        client().subscriptions().pause("sub_1");
        assertEquals("POST", server.requests.getLast().method());
    }
}
