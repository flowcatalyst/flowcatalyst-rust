package io.flowcatalyst.sdk.outbox;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import java.io.InputStream;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.stream.Stream;
import java.util.stream.StreamSupport;
import org.junit.jupiter.api.DynamicTest;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.TestFactory;

/**
 * Runs every case of the shared audit-redaction vectors
 * ({@code src/test/resources/audit-redaction-vectors.json}, a byte-identical
 * copy of the platform repo's {@code docs/spec/audit-redaction-vectors.json}).
 */
class AuditRedactionTest {

    private static final ObjectMapper MAPPER = new ObjectMapper();

    private static JsonNode vectors() throws Exception {
        try (InputStream in = AuditRedactionTest.class.getResourceAsStream("/audit-redaction-vectors.json")) {
            assertNotNull(in, "audit-redaction-vectors.json is on the test classpath");
            return MAPPER.readTree(in);
        }
    }

    private static Set<String> masked(JsonNode testCase) {
        Set<String> out = new HashSet<>();
        testCase.get("masked").forEach(n -> out.add(n.asText()));
        return out;
    }

    @TestFactory
    Stream<DynamicTest> everyVectorRedactsAsExpected() throws Exception {
        JsonNode cases = vectors();
        assertFalse(cases.isEmpty());
        return StreamSupport.stream(cases.spliterator(), false).map(testCase -> DynamicTest.dynamicTest(
                testCase.get("name").asText(),
                () -> assertEquals(
                        testCase.get("expected"),
                        AuditRedaction.redact(testCase.get("input"), masked(testCase)))));
    }

    @Test
    void redactionNeverMutatesItsInput() throws Exception {
        JsonNode input = MAPPER.readTree("{\"password\":\"p\",\"nested\":{\"token\":\"t\"}}");
        JsonNode before = input.deepCopy();
        AuditRedaction.redact(input, Set.of());
        assertEquals(before, input);
    }

    @Test
    void theAuditPayloadCarriesRedactedOperationData() throws Exception {
        OutboxManagerTest.CapturingDriver driver = new OutboxManagerTest.CapturingDriver();
        OutboxManager outbox = new OutboxManager(driver, "clt_TEST123456789");

        Map<String, Object> webhook = new LinkedHashMap<>();
        webhook.put("authType", "HMAC_SIGNATURE");
        webhook.put("signingSecret", "s3cr3t");
        Map<String, Object> data = new LinkedHashMap<>();
        data.put("code", "sa-1");
        data.put("webhookCredentials", webhook);
        data.put("value", "sk_live_123");
        data.put("tags", List.of("a"));

        outbox.createAuditLog(CreateAuditLogDto
                .create("ServiceAccount", "0HZXEQ5Y8JY5Z", "CREATE")
                .withOperationData(data, Set.of("value"))
                .withPrincipalId("0HZXEQ5Y8JY5A"));

        JsonNode payload = MAPPER.readTree(driver.inserted.getFirst().payload());
        JsonNode operationData = MAPPER.readTree(payload.get("operationData").asText());
        assertEquals(
                MAPPER.readTree("{\"code\":\"sa-1\",\"webhookCredentials\":{\"authType\":\"HMAC_SIGNATURE\","
                        + "\"signingSecret\":\"***\"},\"value\":\"***\",\"tags\":[\"a\"]}"),
                operationData);
        assertFalse(payload.toString().contains("s3cr3t"));
        assertFalse(payload.toString().contains("sk_live_123"));
    }

    @Test
    void maskedFieldsSurviveLaterBuilderCalls() throws Exception {
        OutboxManagerTest.CapturingDriver driver = new OutboxManagerTest.CapturingDriver();
        OutboxManager outbox = new OutboxManager(driver, "clt_TEST123456789");

        outbox.createAuditLog(CreateAuditLogDto
                .create("Config", "cfg_1", "SET")
                .withOperationData(Map.of("value", "hidden"), Set.of("value"))
                .withPrincipalId("p")
                .withSource("s")
                .withCorrelationId("c")
                .withApplicationCode("app")
                .withClientCode("clt")
                .withMetadata(Map.of("k", "v"))
                .withHeaders(Map.of("h", "v")));

        JsonNode payload = MAPPER.readTree(driver.inserted.getFirst().payload());
        assertEquals("{\"value\":\"***\"}", payload.get("operationData").asText());
    }
}
