package io.flowcatalyst.sdk.outbox;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import io.flowcatalyst.sdk.tsid.Tsid;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.function.Function;
import org.junit.jupiter.api.Test;

class OutboxManagerTest {

    private static final ObjectMapper MAPPER = new ObjectMapper();

    static final class CapturingDriver implements OutboxDriver {
        final List<OutboxMessage> inserted = new ArrayList<>();
        Object lastTx;

        @Override
        public void insert(OutboxMessage message, Object tx) {
            inserted.add(message);
            lastTx = tx;
        }

        @Override
        public void insertBatch(List<OutboxMessage> messages, Object tx) {
            inserted.addAll(messages);
            lastTx = tx;
        }

        @Override
        public <T> T withTransaction(Function<Object, T> callback) {
            return callback.apply("fake-tx");
        }
    }

    @Test
    void eventMessageMatchesWireContract() throws Exception {
        CapturingDriver driver = new CapturingDriver();
        OutboxManager outbox = new OutboxManager(driver, "clt_TEST123456789");

        String id = outbox.createEvent(CreateEventDto
                .create("orders:sales:order:created", Map.of("orderId", "123"))
                .withSource("order-service")
                .withMessageGroup("order-123")
                .withClientCode("inhance"));

        assertTrue(Tsid.isValid(id));
        OutboxMessage message = driver.inserted.getFirst();
        assertEquals(id, message.id());
        assertEquals(OutboxMessage.MessageType.EVENT, message.type());
        assertEquals("order-123", message.messageGroup());
        assertEquals(OutboxMessage.Status.PENDING, message.status());
        assertEquals("clt_TEST123456789", message.clientId());
        assertEquals(message.payload().getBytes().length, message.payloadSize());
        assertNull(message.headers(), "empty headers must persist as NULL");

        JsonNode payload = MAPPER.readTree(message.payload());
        assertEquals("1.0", payload.get("specVersion").asText());
        assertEquals("orders:sales:order:created", payload.get("type").asText());
        assertEquals("order-service", payload.get("source").asText());
        assertEquals("inhance", payload.get("clientCode").asText());
        // data is embedded as a JSON *string*, matching the wire contract
        assertTrue(payload.get("data").isTextual());
        assertEquals("123", MAPPER.readTree(payload.get("data").asText()).get("orderId").asText());
        assertFalse(payload.has("subject"), "nulls must be omitted");
    }

    @Test
    void unqualifiedCodesAreRejected() {
        assertThrows(IllegalArgumentException.class,
                () -> CreateEventDto.create("order-created", Map.of()));
        assertThrows(IllegalArgumentException.class,
                () -> CreateDispatchJobDto.create("src", "a::b:c", "https://t", "{}", "pool"));
    }

    @Test
    void dispatchJobPayloadCarriesDefaultsAndMode() throws Exception {
        CapturingDriver driver = new CapturingDriver();
        OutboxManager outbox = new OutboxManager(driver, "clt_TEST123456789");

        outbox.createDispatchJob(CreateDispatchJobDto
                .create("svc", "app:sub:agg:act", "https://example.com/hook", "{\"k\":1}", "pool-1")
                .withMode(CreateDispatchJobDto.DispatchMode.BLOCK_ON_ERROR)
                .withMessageGroup("group-1"));

        OutboxMessage message = driver.inserted.getFirst();
        assertEquals(OutboxMessage.MessageType.DISPATCH_JOB, message.type());
        JsonNode payload = MAPPER.readTree(message.payload());
        assertEquals("application/json", payload.get("payloadContentType").asText());
        assertTrue(payload.get("dataOnly").asBoolean());
        assertEquals(30, payload.get("timeoutSeconds").asInt());
        assertEquals(5, payload.get("maxRetries").asInt());
        assertEquals("BLOCK_ON_ERROR", payload.get("mode").asText());
        assertEquals("group-1", payload.get("messageGroup").asText());
    }

    @Test
    void auditLogDefaultsPerformedAtAndStringifiesOperationData() throws Exception {
        CapturingDriver driver = new CapturingDriver();
        OutboxManager outbox = new OutboxManager(driver, "clt_TEST123456789");

        outbox.createAuditLog(CreateAuditLogDto
                .create("User", "prn_ABC", "CREATE")
                .withOperationData(Map.of("email", "a@b.com"))
                .withApplicationCode("example"));

        OutboxMessage message = driver.inserted.getFirst();
        assertEquals(OutboxMessage.MessageType.AUDIT_LOG, message.type());
        assertNull(message.messageGroup());
        JsonNode payload = MAPPER.readTree(message.payload());
        assertTrue(payload.hasNonNull("performedAt"));
        assertTrue(payload.get("operationData").isTextual());
        assertEquals("example", payload.get("applicationCode").asText());
    }

    @Test
    void batchCreatesOneMessagePerDtoAndPassesTxThrough() {
        CapturingDriver driver = new CapturingDriver();
        OutboxManager outbox = new OutboxManager(driver, "clt_TEST123456789");

        Object tx = new Object();
        List<String> ids = outbox.createEvents(List.of(
                CreateEventDto.create("a:b:c:d", Map.of()),
                CreateEventDto.create("a:b:c:e", Map.of())), tx);

        assertEquals(2, ids.size());
        assertEquals(2, driver.inserted.size());
        assertEquals(tx, driver.lastTx);
        assertEquals(List.of(), outbox.createEvents(List.of()));
    }

    @Test
    void missingClientIdIsRejectedAtWriteTime() {
        OutboxManager outbox = new OutboxManager(new CapturingDriver(), "");
        assertThrows(IllegalStateException.class,
                () -> outbox.createEvent(CreateEventDto.create("a:b:c:d", Map.of())));
    }
}
