package io.flowcatalyst.sdk;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertInstanceOf;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.flowcatalyst.sdk.error.FlowCatalystException;
import io.flowcatalyst.sdk.error.SdkError;
import io.flowcatalyst.sdk.generated.model.ApplicationProvisionServiceAccountResponse;
import io.flowcatalyst.sdk.generated.model.AuditLogListResponse;
import io.flowcatalyst.sdk.generated.model.ClientConfigResponse;
import io.flowcatalyst.sdk.generated.model.CreateRoleRequest;
import io.flowcatalyst.sdk.generated.model.CreateScheduledJobRequest;
import io.flowcatalyst.sdk.generated.model.OffsetPageScheduledJobResponse;
import io.flowcatalyst.sdk.generated.model.ServiceAccountResponse;
import io.flowcatalyst.sdk.generated.model.UpdateConnectionRequest;
import io.flowcatalyst.sdk.generated.model.UpdateEventTypeRequest;
import io.flowcatalyst.sdk.resources.AuditLogsResource;
import io.flowcatalyst.sdk.resources.PrincipalsResource;
import io.flowcatalyst.sdk.resources.RouterResource;
import io.flowcatalyst.sdk.resources.ScheduledJobsResource;
import java.util.List;
import java.util.Map;
import java.util.stream.Collectors;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

/**
 * The hand-written resources against the Go platform's wire shapes
 * ({@code frontend/openapi/openapi.json}, Go's {@code api/openapi.lock.json}):
 * paths, query members, request bodies, and how each response is read.
 */
class GoApiWireTest {

    private static final String APPLICATION = "{\"id\":\"app_1\",\"code\":\"orders\",\"name\":\"Orders\","
            + "\"type\":\"APPLICATION\",\"active\":true,\"hasLoginClient\":false,%s"
            + "\"createdAt\":\"2026-01-01T00:00:00Z\",\"updatedAt\":\"2026-01-01T00:00:00Z\"}";

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

    private List<StubServer.Recorded> apiCalls() {
        return server.requests.stream()
                .filter(r -> r.pathAndQuery().startsWith("/api/"))
                .collect(Collectors.toList());
    }

    private static String call(StubServer.Recorded r) {
        return r.method() + " " + r.pathAndQuery();
    }

    // ── applications ────────────────────────────────────────────────

    @Test
    void getServiceAccountReadsTheApplicationThenTheServiceAccount() {
        server.on("GET", "/api/applications/app_1", 200,
                String.format(APPLICATION, "\"serviceAccountId\":\"sa_1\","));
        server.on("GET", "/api/service-accounts/sa_1", 200,
                "{\"id\":\"sa_1\",\"code\":\"orders-sa\",\"name\":\"Orders SA\",\"active\":true,"
                        + "\"authType\":\"BEARER\",\"clientIds\":[],\"roles\":[],"
                        + "\"createdAt\":\"2026-01-01T00:00:00Z\",\"updatedAt\":\"2026-01-01T00:00:00Z\"}");

        ServiceAccountResponse sa = client().applications().getServiceAccount("app_1");

        assertEquals("orders-sa", sa.getCode());
        assertEquals(List.of("GET /api/applications/app_1", "GET /api/service-accounts/sa_1"),
                apiCalls().stream().map(GoApiWireTest::call).toList());
    }

    @Test
    void getServiceAccountIsNotFoundWhenTheApplicationHasNone() {
        server.on("GET", "/api/applications/app_1", 200, String.format(APPLICATION, ""));

        FlowCatalystException e = assertThrows(FlowCatalystException.class,
                () -> client().applications().getServiceAccount("app_1"));

        assertInstanceOf(SdkError.NotFound.class, e.error());
        assertEquals(1, apiCalls().size());
    }

    @Test
    void getClientConfigReadsOneClientsConfig() {
        server.on("GET", "/api/applications/app_1/clients/clt_1", 200,
                "{\"id\":\"cfg_1\",\"applicationId\":\"app_1\",\"clientId\":\"clt_1\",\"enabled\":true,"
                        + "\"configJson\":{\"theme\":\"dark\"},"
                        + "\"createdAt\":\"2026-01-01T00:00:00Z\",\"updatedAt\":\"2026-01-01T00:00:00Z\"}");

        ClientConfigResponse config = client().applications().getClientConfig("app_1", "clt_1");

        assertTrue(config.getEnabled());
        assertEquals(Map.of("theme", "dark"), config.getConfigJson());
    }

    @Test
    void provisionServiceAccountKeepsTheNestedOneTimeSecret() {
        server.on("POST", "/api/applications/app_1/provision-service-account", 201,
                "{\"message\":\"ok\",\"serviceAccount\":{\"principalId\":\"prn_1\",\"name\":\"Orders SA\","
                        + "\"oauthClient\":{\"id\":\"oac_1\",\"clientId\":\"orders-sa\",\"clientSecret\":\"s3cret\"}}}");

        ApplicationProvisionServiceAccountResponse result =
                client().applications().provisionServiceAccount("app_1");

        assertEquals("s3cret", result.getServiceAccount().getOauthClient().getClientSecret());
    }

    @Test
    void applicationRolesAreNames() {
        server.on("GET", "/api/applications/by-id/app_1/roles", 200, "{\"roles\":[\"orders:admin\"]}");

        assertEquals(List.of("orders:admin"), client().applications().listRoles("app_1").getRoles());
    }

    // ── dispatch pools ──────────────────────────────────────────────

    @Test
    void dispatchPoolsListReadsPoolsAndArchiveIsAPost() {
        server.on("GET", "/api/dispatch-pools", 200,
                "{\"pools\":[{\"id\":\"dp_1\",\"code\":\"default\",\"name\":\"Default\",\"concurrency\":5,"
                        + "\"status\":\"ACTIVE\",\"createdAt\":\"2026-01-01T00:00:00Z\","
                        + "\"updatedAt\":\"2026-01-01T00:00:00Z\"}],\"total\":1}");
        server.on("POST", "/api/dispatch-pools/dp_1/archive", 204, null);

        FlowCatalystClient client = client();
        assertEquals("default", client.dispatchPools().list(null, null).getPools().get(0).getCode());
        client.dispatchPools().archive("dp_1");

        assertEquals("POST /api/dispatch-pools/dp_1/archive", call(apiCalls().get(1)));
    }

    // ── event types / connections ───────────────────────────────────

    @Test
    void eventTypeUpdateAlwaysSendsName() {
        server.on("GET", "/api/event-types/evt_1", 200,
                "{\"id\":\"evt_1\",\"code\":\"orders:sales:order:created\",\"name\":\"Order Created\","
                        + "\"eventName\":\"created\",\"application\":\"orders\",\"subdomain\":\"sales\","
                        + "\"aggregate\":\"order\",\"source\":\"API\",\"status\":\"CURRENT\",\"specVersions\":[],"
                        + "\"createdAt\":\"2026-01-01T00:00:00Z\",\"updatedAt\":\"2026-01-01T00:00:00Z\"}");
        server.on("PUT", "/api/event-types/evt_1", 204, null);

        client().eventTypes().update("evt_1", new UpdateEventTypeRequest().description("d"));

        List<StubServer.Recorded> calls = apiCalls();
        assertEquals("GET /api/event-types/evt_1", call(calls.get(0)));
        assertTrue(calls.get(1).body().contains("\"name\":\"Order Created\""), calls.get(1).body());
    }

    @Test
    void eventTypeArchiveAndDeleteUseTheDeleteRoute() {
        server.on("DELETE", "/api/event-types/evt_1", 204, null);

        FlowCatalystClient client = client();
        client.eventTypes().delete("evt_1");
        client.eventTypes().archive("evt_1");

        assertEquals(List.of("DELETE /api/event-types/evt_1", "DELETE /api/event-types/evt_1"),
                apiCalls().stream().map(GoApiWireTest::call).toList());
    }

    @Test
    void connectionUpdateAlwaysSendsName() {
        server.on("GET", "/api/connections/con_1", 200,
                "{\"id\":\"con_1\",\"code\":\"hook\",\"name\":\"Hook\",\"status\":\"ACTIVE\","
                        + "\"serviceAccountId\":\"sa_1\",\"source\":\"API\","
                        + "\"createdAt\":\"2026-01-01T00:00:00Z\",\"updatedAt\":\"2026-01-01T00:00:00Z\"}");
        server.on("PUT", "/api/connections/con_1", 204, null);

        FlowCatalystClient client = client();
        client.connections().update("con_1", new UpdateConnectionRequest().description("d"));
        client.connections().update("con_1", new UpdateConnectionRequest().name("Renamed"));

        List<StubServer.Recorded> calls = apiCalls();
        assertEquals(3, calls.size(), "only the update without a name reads the connection");
        assertTrue(calls.get(1).body().contains("\"name\":\"Hook\""), calls.get(1).body());
        assertTrue(calls.get(2).body().contains("\"name\":\"Renamed\""), calls.get(2).body());
    }

    // ── list filters ────────────────────────────────────────────────

    @Test
    void auditLogIdFiltersAreCommaSeparatedAndPagingIsByCursor() {
        server.on("GET", "/api/audit-logs", 200,
                "{\"auditLogs\":[],\"hasMore\":true,\"nextCursor\":\"cur_2\"}");

        AuditLogListResponse page = client().auditLogs().list(new AuditLogsResource.Filters(
                "cur_1", 50, null, null, null, null, List.of("app_1", "app_2"), List.of("clt_1")));

        assertEquals("GET /api/audit-logs?after=cur_1&pageSize=50&applicationIds=app_1%2Capp_2&clientIds=clt_1",
                call(apiCalls().get(0)));
        assertTrue(page.getHasMore());
        assertEquals("cur_2", page.getNextCursor());
    }

    @Test
    void principalRoleFilterIsCommaSeparatedAndEmailSearchUsesQ() {
        server.on("GET", "/api/principals", 200, "{\"principals\":[],\"total\":0}");

        FlowCatalystClient client = client();
        client.principals().list(new PrincipalsResource.Filters(
                null, null, null, null, List.of("orders:admin", "orders:viewer"), null, null, null, null));
        client.principals().listByEmail("ann@acme.test");

        List<StubServer.Recorded> calls = apiCalls();
        assertEquals("GET /api/principals?roles=orders%3Aadmin%2Corders%3Aviewer", call(calls.get(0)));
        assertEquals("GET /api/principals?q=ann%40acme.test", call(calls.get(1)));
    }

    // ── creates with required booleans ──────────────────────────────

    @Test
    void createsSendTheRequiredBooleans() {
        server.on("POST", "/api/roles", 201, "{\"id\":\"rol_1\"}");
        server.on("POST", "/api/scheduled-jobs", 201, "{\"id\":\"sj_1\"}");

        FlowCatalystClient client = client();
        assertEquals("rol_1", client.roles().create(new CreateRoleRequest()
                .applicationCode("orders").roleName("admin").displayName("Admin")).getId());
        assertEquals("sj_1", client.scheduledJobs().create(new CreateScheduledJobRequest()
                .code("nightly").name("Nightly").crons(List.of("0 0 2 * * *"))).getId());

        List<StubServer.Recorded> calls = apiCalls();
        assertTrue(calls.get(0).body().contains("\"clientManaged\":false"), calls.get(0).body());
        assertTrue(calls.get(1).body().contains("\"concurrent\":false"), calls.get(1).body());
        assertTrue(calls.get(1).body().contains("\"tracksCompletion\":false"), calls.get(1).body());
    }

    // ── scheduled jobs ──────────────────────────────────────────────

    @Test
    void scheduledJobsReadTotalPagesAndLookUpByCodeWithinAClient() {
        server.on("GET", "/api/scheduled-jobs", 200,
                "{\"data\":[],\"page\":1,\"size\":20,\"total\":41,\"total_pages\":3}");
        server.on("GET", "/api/scheduled-jobs/by-code/nightly", 404, "{\"error\":\"NOT_FOUND\"}");

        FlowCatalystClient client = client();
        OffsetPageScheduledJobResponse page = client.scheduledJobs().list(ScheduledJobsResource.ListFilters.none());
        assertThrows(FlowCatalystException.class, () -> client.scheduledJobs().getByCode("nightly", "clt_1"));

        assertEquals(3L, page.getTotalPages());
        assertEquals("GET /api/scheduled-jobs/by-code/nightly?clientId=clt_1", call(apiCalls().get(1)));
    }

    // ── router ──────────────────────────────────────────────────────

    @Test
    void inPipelineExposesTheTopLevelPoolAndQueueAsDetail() throws Exception {
        try (StubServer router = new StubServer()) {
            router.on("GET", "/monitoring/in-flight-messages/check", r -> r.pathAndQuery().contains("m1")
                    ? StubServer.Reply.json(200,
                            "{\"messageId\":\"m1\",\"inPipeline\":true,\"poolCode\":\"default\",\"queueId\":\"q1\"}")
                    : StubServer.Reply.json(200, "{\"messageId\":\"m2\",\"inPipeline\":false}"));
            FlowCatalystClient client = FlowCatalystClient.builder()
                    .baseUrl(server.baseUrl())
                    .routerBaseUrl(router.baseUrl())
                    .clientCredentials("id", "secret")
                    .build();

            RouterResource.InPipelineCheckResponse hit = client.router().inPipeline("m1");
            RouterResource.InPipelineCheckResponse miss = client.router().inPipeline("m2");

            assertTrue(hit.inPipeline());
            assertEquals("default", hit.poolCode());
            assertEquals("default", hit.detail().poolCode());
            assertEquals("q1", hit.detail().queueId());
            assertEquals("m1", hit.detail().messageId());
            assertNull(miss.detail());
        }
    }
}
