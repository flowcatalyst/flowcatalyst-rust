package io.flowcatalyst.sdk.outbox;

import com.fasterxml.jackson.databind.ObjectMapper;
import io.flowcatalyst.sdk.http.Json;
import java.sql.Connection;
import java.sql.PreparedStatement;
import java.sql.SQLException;
import java.sql.Timestamp;
import java.sql.Types;
import java.util.List;
import java.util.function.Function;
import javax.sql.DataSource;

/**
 * Outbox driver for JDBC data sources (PostgreSQL and MySQL).
 *
 * <p>Transactional outbox usage — the {@code tx} handle is a plain
 * {@link Connection}:
 *
 * <pre>{@code
 * var driver = new JdbcOutboxDriver(dataSource);
 * var outbox = new OutboxManager(driver, "clt_0HZXEQ5Y8JY5Z");
 *
 * driver.withTransaction(tx -> {
 *     var conn = (Connection) tx;
 *     // business writes on conn ...
 *     outbox.createEvent(orderShipped, tx); // same transaction
 *     return null;
 * });
 * }</pre>
 */
public final class JdbcOutboxDriver implements OutboxDriver {

    private static final String INSERT_SQL = """
            INSERT INTO %s (
              id, type, message_group, payload, status, created_at, updated_at,
              client_id, payload_size, headers
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            """;

    private final DataSource dataSource;
    private final String insertSql;
    private final ObjectMapper mapper = Json.newMapper();

    public JdbcOutboxDriver(DataSource dataSource) {
        this(dataSource, "outbox_messages");
    }

    public JdbcOutboxDriver(DataSource dataSource, String table) {
        this.dataSource = dataSource;
        this.insertSql = INSERT_SQL.formatted(table);
    }

    @Override
    public void insert(OutboxMessage message, Object tx) {
        if (tx instanceof Connection connection) {
            insertOn(connection, List.of(message));
            return;
        }
        try (Connection connection = dataSource.getConnection()) {
            insertOn(connection, List.of(message));
        } catch (SQLException e) {
            throw new OutboxPersistenceException("Outbox insert failed", e);
        }
    }

    @Override
    public void insertBatch(List<OutboxMessage> messages, Object tx) {
        if (messages.isEmpty()) return;

        if (tx instanceof Connection connection) {
            insertOn(connection, messages);
            return;
        }

        // Own short-lived transaction so the batch is atomic.
        try (Connection connection = dataSource.getConnection()) {
            boolean previousAutoCommit = connection.getAutoCommit();
            connection.setAutoCommit(false);
            try {
                insertOn(connection, messages);
                connection.commit();
            } catch (RuntimeException | SQLException e) {
                connection.rollback();
                throw e instanceof RuntimeException re
                        ? re : new OutboxPersistenceException("Outbox batch insert failed", e);
            } finally {
                connection.setAutoCommit(previousAutoCommit);
            }
        } catch (SQLException e) {
            throw new OutboxPersistenceException("Outbox batch insert failed", e);
        }
    }

    @Override
    public <T> T withTransaction(Function<Object, T> callback) {
        try (Connection connection = dataSource.getConnection()) {
            boolean previousAutoCommit = connection.getAutoCommit();
            connection.setAutoCommit(false);
            try {
                T result = callback.apply(connection);
                connection.commit();
                return result;
            } catch (RuntimeException e) {
                connection.rollback();
                throw e;
            } finally {
                connection.setAutoCommit(previousAutoCommit);
            }
        } catch (SQLException e) {
            throw new OutboxPersistenceException("Outbox transaction failed", e);
        }
    }

    private void insertOn(Connection connection, List<OutboxMessage> messages) {
        try (PreparedStatement statement = connection.prepareStatement(insertSql)) {
            // PostgreSQL's jsonb has no implicit cast from varchar, so JSON
            // params must be bound as Types.OTHER there; other databases
            // (H2, MySQL) take a plain string.
            boolean postgres = connection.getMetaData().getDatabaseProductName()
                    .toLowerCase(java.util.Locale.ROOT).contains("postgres");
            for (OutboxMessage message : messages) {
                bind(statement, message, postgres);
                statement.addBatch();
            }
            statement.executeBatch();
        } catch (SQLException e) {
            throw new OutboxPersistenceException("Outbox insert failed", e);
        }
    }

    private void bind(PreparedStatement statement, OutboxMessage message, boolean postgres)
            throws SQLException {
        statement.setString(1, message.id());
        statement.setString(2, message.type().name());
        statement.setString(3, message.messageGroup());
        statement.setString(4, message.payload());
        statement.setInt(5, message.status());
        statement.setTimestamp(6, Timestamp.from(message.createdAt()));
        statement.setTimestamp(7, Timestamp.from(message.updatedAt()));
        statement.setString(8, message.clientId());
        statement.setInt(9, message.payloadSize());
        if (message.headers() == null) {
            statement.setNull(10, postgres ? Types.OTHER : Types.VARCHAR);
        } else {
            String json;
            try {
                json = mapper.writeValueAsString(message.headers());
            } catch (com.fasterxml.jackson.core.JsonProcessingException e) {
                throw new OutboxPersistenceException("Failed to serialize outbox headers", e);
            }
            if (postgres) {
                statement.setObject(10, json, Types.OTHER);
            } else {
                statement.setString(10, json);
            }
        }
    }

    /** Wraps SQL failures from the driver. */
    public static final class OutboxPersistenceException extends RuntimeException {
        public OutboxPersistenceException(String message, Throwable cause) {
            super(message, cause);
        }
    }
}
