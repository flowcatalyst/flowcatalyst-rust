package io.flowcatalyst.sdk.outbox;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.sql.Connection;
import java.sql.DriverManager;
import java.sql.ResultSet;
import java.sql.SQLException;
import java.sql.Statement;
import java.time.Instant;
import java.util.List;
import java.util.Map;
import javax.sql.DataSource;
import org.h2.jdbcx.JdbcDataSource;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.condition.EnabledIfEnvironmentVariable;

/**
 * Driver semantics against in-memory H2 (always), plus the same assertions
 * against real PostgreSQL when FC_JAVA_SDK_TEST_PG_URL is set (e.g.
 * jdbc:postgresql://localhost:15432/postgres?user=postgres).
 */
class JdbcOutboxDriverTest {

    private JdbcDataSource h2;

    @BeforeEach
    void setUp() throws Exception {
        h2 = new JdbcDataSource();
        h2.setURL("jdbc:h2:mem:outbox" + System.nanoTime() + ";DB_CLOSE_DELAY=-1");
        try (Connection c = h2.getConnection(); Statement s = c.createStatement()) {
            s.execute("""
                    CREATE TABLE outbox_messages (
                        id VARCHAR(26) PRIMARY KEY,
                        type VARCHAR(20) NOT NULL,
                        message_group VARCHAR(255),
                        payload TEXT NOT NULL,
                        status SMALLINT NOT NULL DEFAULT 0,
                        retry_count SMALLINT NOT NULL DEFAULT 0,
                        created_at TIMESTAMP WITH TIME ZONE NOT NULL,
                        updated_at TIMESTAMP WITH TIME ZONE NOT NULL,
                        error_message TEXT,
                        client_id VARCHAR(26),
                        payload_size INTEGER,
                        headers TEXT
                    )""");
        }
    }

    private static OutboxMessage message(String id, String group, Map<String, String> headers) {
        Instant now = Instant.now();
        return new OutboxMessage(id, OutboxMessage.MessageType.EVENT, group, "{\"k\":1}",
                OutboxMessage.Status.PENDING, now, now, "clt_TEST", 7, headers);
    }

    private int count(DataSource ds) throws SQLException {
        try (Connection c = ds.getConnection();
                ResultSet rs = c.createStatement()
                        .executeQuery("SELECT COUNT(*) FROM outbox_messages")) {
            rs.next();
            return rs.getInt(1);
        }
    }

    @Test
    void insertWritesRowWithHeadersJson() throws Exception {
        new JdbcOutboxDriver(h2).insert(message("id-1", "g1", Map.of("x", "y")), null);

        try (Connection c = h2.getConnection();
                ResultSet rs = c.createStatement().executeQuery(
                        "SELECT type, message_group, status, client_id, headers "
                                + "FROM outbox_messages WHERE id = 'id-1'")) {
            assertTrue(rs.next());
            assertEquals("EVENT", rs.getString(1));
            assertEquals("g1", rs.getString(2));
            assertEquals(0, rs.getInt(3));
            assertEquals("clt_TEST", rs.getString(4));
            assertEquals("{\"x\":\"y\"}", rs.getString(5));
        }
    }

    @Test
    void insertBatchIsAtomicWithoutCallerTx() throws Exception {
        JdbcOutboxDriver driver = new JdbcOutboxDriver(h2);
        // Second message violates the PK → whole batch must roll back.
        assertThrows(JdbcOutboxDriver.OutboxPersistenceException.class, () ->
                driver.insertBatch(List.of(
                        message("dup", null, null), message("dup", null, null)), null));
        assertEquals(0, count(h2));

        driver.insertBatch(List.of(message("a", null, null), message("b", null, null)), null);
        assertEquals(2, count(h2));
    }

    @Test
    void withTransactionCommitsOrRollsBackAtomically() throws Exception {
        JdbcOutboxDriver driver = new JdbcOutboxDriver(h2);
        OutboxManager outbox = new OutboxManager(driver, "clt_TEST");

        // Business write + outbox write in one tx, rolled back together.
        assertThrows(IllegalStateException.class, () -> driver.withTransaction(tx -> {
            outbox.createEvent(CreateEventDto.create("a:b:c:d", Map.of()), tx);
            throw new IllegalStateException("business failure");
        }));
        assertEquals(0, count(h2));

        driver.withTransaction(tx -> {
            outbox.createEvent(CreateEventDto.create("a:b:c:d", Map.of()), tx);
            outbox.createEvent(CreateEventDto.create("a:b:c:e", Map.of()), tx);
            return null;
        });
        assertEquals(2, count(h2));
    }

    @Test
    @EnabledIfEnvironmentVariable(named = "FC_JAVA_SDK_TEST_PG_URL", matches = ".+")
    void worksAgainstRealPostgres() throws Exception {
        String url = System.getenv("FC_JAVA_SDK_TEST_PG_URL");
        String table = "outbox_messages_javasdk_test";
        try (Connection c = DriverManager.getConnection(url);
                Statement s = c.createStatement()) {
            s.execute("DROP TABLE IF EXISTS " + table);
            s.execute("""
                    CREATE TABLE %s (
                        id VARCHAR(26) PRIMARY KEY,
                        type VARCHAR(20) NOT NULL,
                        message_group VARCHAR(255),
                        payload TEXT NOT NULL,
                        status SMALLINT NOT NULL DEFAULT 0,
                        retry_count SMALLINT NOT NULL DEFAULT 0,
                        created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                        updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                        error_message TEXT,
                        client_id VARCHAR(26),
                        payload_size INTEGER,
                        headers JSONB
                    )""".formatted(table));
        }

        org.postgresql.ds.PGSimpleDataSource pg = new org.postgresql.ds.PGSimpleDataSource();
        pg.setURL(url);
        JdbcOutboxDriver driver = new JdbcOutboxDriver(pg, table);
        driver.insert(message("pg-1", "g", Map.of("h", "v")), null);
        driver.insertBatch(List.of(message("pg-2", null, null), message("pg-3", null, null)), null);

        try (Connection c = DriverManager.getConnection(url);
                ResultSet rs = c.createStatement()
                        .executeQuery("SELECT COUNT(*) FROM " + table)) {
            rs.next();
            assertEquals(3, rs.getInt(1));
        } finally {
            try (Connection c = DriverManager.getConnection(url);
                    Statement s = c.createStatement()) {
                s.execute("DROP TABLE " + table);
            }
        }
    }
}
