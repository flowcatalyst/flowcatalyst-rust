package examples;

import io.flowcatalyst.sdk.outbox.CreateEventDto;
import io.flowcatalyst.sdk.outbox.JdbcOutboxDriver;
import io.flowcatalyst.sdk.outbox.OutboxManager;
import java.sql.Connection;
import java.sql.PreparedStatement;
import java.util.Map;
import javax.sql.DataSource;
import org.postgresql.ds.PGSimpleDataSource;

/**
 * Transactional-outbox example: a business write and its event land in the
 * SAME database transaction, so either both commit or neither does. The
 * outbox poller picks the event up asynchronously.
 *
 * <pre>
 * export FC_DB_URL="jdbc:postgresql://localhost:5432/app?user=app&password=..."
 * export FC_CLIENT_ID_TSID=clt_...   # your FlowCatalyst client (tenant) id
 * mvn -q test-compile org.codehaus.mojo:exec-maven-plugin:3.5.0:java \
 *     -Dexec.mainClass=examples.OrderService -Dexec.classpathScope=test
 * </pre>
 */
public final class OrderService {

    public static void main(String[] args) {
        PGSimpleDataSource dataSource = new PGSimpleDataSource();
        dataSource.setURL(System.getenv("FC_DB_URL"));

        String orderId = placeOrder(dataSource, System.getenv("FC_CLIENT_ID_TSID"));
        System.out.println("Order " + orderId + " placed; event committed atomically.");
    }

    static String placeOrder(DataSource dataSource, String clientId) {
        JdbcOutboxDriver driver = new JdbcOutboxDriver(dataSource);
        OutboxManager outbox = new OutboxManager(driver, clientId);

        return driver.withTransaction(tx -> {
            Connection connection = (Connection) tx;
            String orderId = io.flowcatalyst.sdk.tsid.Tsid.generateWithPrefix("ord");

            try (PreparedStatement insert = connection.prepareStatement(
                    "INSERT INTO orders (id, status) VALUES (?, 'PLACED')")) {
                insert.setString(1, orderId);
                insert.executeUpdate();
            } catch (java.sql.SQLException e) {
                throw new RuntimeException(e);
            }

            outbox.createEvent(CreateEventDto
                    .create("orders:sales:order:placed", Map.of("orderId", orderId))
                    .withSource("order-service")
                    .withMessageGroup(orderId), tx);

            return orderId;
        });
    }

    private OrderService() {}
}
