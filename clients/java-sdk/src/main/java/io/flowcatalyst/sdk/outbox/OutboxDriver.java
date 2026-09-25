package io.flowcatalyst.sdk.outbox;

import java.util.List;
import java.util.function.Function;

/**
 * Driver interface for outbox persistence.
 *
 * <p>Implementations write outbox rows. To make outbox writes atomic with the
 * caller's business writes, pass the same transaction handle (opaque to the
 * SDK; for the JDBC driver, a {@link java.sql.Connection}) to both: your
 * repository's persist call and the driver's insert call.
 *
 * <ul>
 *   <li>{@code insert} / {@code insertBatch} accept an optional {@code tx}
 *       handle. If null, the driver writes against its default executor
 *       (typically a pool). If present, the driver writes against the
 *       caller-supplied transaction so the row is part of the same tx as the
 *       business writes.</li>
 *   <li>{@code withTransaction} is optional; implementations that support it
 *       open a tx, run the callback against the handle, then commit or roll
 *       back.</li>
 * </ul>
 *
 * The bundled {@link JdbcOutboxDriver} implements all methods against a
 * {@link javax.sql.DataSource}. Most consumers should use it directly.
 */
public interface OutboxDriver {

    /**
     * Insert a single message into the outbox. If {@code tx} is provided, the
     * write joins that transaction; otherwise the driver writes via its
     * default executor.
     */
    void insert(OutboxMessage message, Object tx);

    /**
     * Insert multiple messages into the outbox. If {@code tx} is provided,
     * all rows join that transaction; otherwise the driver opens its own
     * short-lived transaction so the batch is atomic.
     */
    void insertBatch(List<OutboxMessage> messages, Object tx);

    /**
     * Open a transaction, run the callback against the tx handle, and commit
     * (or roll back on throw). Optional — drivers that don't support it throw
     * {@link UnsupportedOperationException}.
     */
    default <T> T withTransaction(Function<Object, T> callback) {
        throw new UnsupportedOperationException(
                "This driver does not support orchestrated transactions");
    }
}
