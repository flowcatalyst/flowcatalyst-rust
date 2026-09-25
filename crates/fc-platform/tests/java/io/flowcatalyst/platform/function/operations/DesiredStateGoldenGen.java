package io.flowcatalyst.platform.function.operations;

import io.flowcatalyst.platform.function.CorruptFunctionVersionException;
import io.flowcatalyst.platform.function.DnsLabel;
import io.flowcatalyst.platform.function.FunctionHostRepository;
import io.flowcatalyst.platform.function.FunctionRepository;
import io.flowcatalyst.platform.function.FunctionRouteRepository;
import io.flowcatalyst.platform.function.FunctionSettingsRepository;
import io.flowcatalyst.platform.function.FunctionVersionRepository;
import io.flowcatalyst.platform.serviceaccount.ServiceAccountRepository;
import io.flowcatalyst.platform.shared.database.Migrator;
import io.flowcatalyst.platform.shared.encryption.Encryption;
import io.flowcatalyst.platform.shared.json.Json;
import org.postgresql.ds.PGSimpleDataSource;
import tools.jackson.databind.node.ObjectNode;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.sql.Connection;
import java.sql.Statement;
import java.time.Instant;
import java.util.HexFormat;
import java.util.List;
import java.util.Optional;

/// Golden-file generator for the desired-state document (P6). Runs Java's
/// REAL `DesiredState.build` against a Postgres migrated by Java's own
/// `Migrator` and loaded with `tests/data/function/desired-state-fixture.sql`,
/// and writes, per pool, the exact body bytes (`Json.write(document)`, as
/// `FunctionControlApi.desiredState` writes them) and the `ETag`, or the
/// corrupt-row refusal, to `tests/data/function/desired-state-golden.json`.
///
/// Not part of any build. Classpath: the Java repo's compiled classes, with
/// the classes whose sources changed after the pin `0118cdca` recompiled
/// from the pinned sources and put first (`git show 0118cdca:<file>`, for
/// every file on `DesiredState.build`'s path that differs: `Manifest`,
/// `FunctionVersionRepository`, `Hostname`, `RoutePattern`,
/// `FunctionAddressPattern`, `ServiceAccountRepository`,
/// `OutboundCredentials`, `Encryption`):
///
/// ```
/// J=../flowcatalyst-javalin; M=~/.m2/repository; P=/tmp/pinned
/// DEPS=$M/tools/jackson/core/jackson-databind/3.1.5/jackson-databind-3.1.5.jar:$M/tools/jackson/core/jackson-core/3.1.5/jackson-core-3.1.5.jar:$M/com/fasterxml/jackson/core/jackson-annotations/2.22/jackson-annotations-2.22.jar:$M/org/slf4j/slf4j-api/2.0.18/slf4j-api-2.0.18.jar:$M/org/jooq/jooq/3.21.7/jooq-3.21.7.jar:$M/io/r2dbc/r2dbc-spi/1.0.0.RELEASE/r2dbc-spi-1.0.0.RELEASE.jar:$M/org/reactivestreams/reactive-streams/1.0.4/reactive-streams-1.0.4.jar:$M/org/postgresql/postgresql/42.7.13/postgresql-42.7.13.jar:$M/org/flywaydb/flyway-core/13.3.0/flyway-core-13.3.0.jar:$M/org/flywaydb/flyway-database-postgresql/13.3.0/flyway-database-postgresql-13.3.0.jar
/// CP=$J/server/target/classes:$J/usecase/target/classes:$J/sdk/target/classes:$J/function-api/target/classes:$DEPS
/// # the pinned sources of the changed files, compiled over the checkout's classes:
/// (cd $J && for f in <the files above>; do git show 0118cdca:$f > $P/src/$(basename $f); done)
/// javac -proc:none -d $P/classes -cp $CP $P/src/*.java
/// javac -proc:none -d /tmp/golden -cp $P/classes:$CP \
///     crates/fc-platform/tests/java/io/flowcatalyst/platform/function/operations/DesiredStateGoldenGen.java
/// docker run -d --rm -p 55432:5432 -e POSTGRES_PASSWORD=test -e POSTGRES_DB=golden postgres:16
/// java -cp /tmp/golden:$P/classes:$CP io.flowcatalyst.platform.function.operations.DesiredStateGoldenGen \
///     jdbc:postgresql://localhost:55432/golden postgres test \
///     crates/fc-platform/tests/data/function/desired-state-fixture.sql \
///     crates/fc-platform/tests/data/function/desired-state-golden.json
/// ```
public final class DesiredStateGoldenGen {

    /// The fixture's instant: the live window (45 s) is measured from here.
    static final Instant NOW = Instant.parse("2026-09-25T12:00:00Z");

    /// The bytes of "0123456789abcdef0123456789abcdef"; the fixture's
    /// `encrypted:` values are sealed with it.
    static final String APP_KEY = "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";

    static final List<String> POOLS = List.of("edge", "batch", "other", "broken");

    public static void main(String[] args) throws Exception {
        PGSimpleDataSource ds = new PGSimpleDataSource();
        ds.setUrl(args[0]);
        ds.setUser(args[1]);
        ds.setPassword(args[2]);
        Migrator.migrate(ds);
        String fixture = Files.readString(Path.of(args[3]), StandardCharsets.UTF_8);
        try (Connection c = ds.getConnection(); Statement st = c.createStatement()) {
            st.execute(fixture);
        }

        Optional<Encryption> encryption = Optional.of(Encryption.withKey(APP_KEY));
        DesiredState desired = new DesiredState(new FunctionRepository(ds), new FunctionVersionRepository(ds),
                new FunctionHostRepository(ds), new ServiceAccountRepository(ds, encryption),
                new FunctionSettingsRepository(ds, encryption), new FunctionRouteRepository(ds));

        ObjectNode out = Json.MAPPER.createObjectNode();
        out.put("now", NOW.toString());
        out.put("appKey", APP_KEY);
        ObjectNode pools = out.putObject("pools");
        for (String pool : POOLS) {
            ObjectNode entry = pools.putObject(pool);
            try {
                DesiredState.Document document = desired.build(new DnsLabel(pool), NOW);
                byte[] body = Json.write(document).getBytes(StandardCharsets.UTF_8);
                entry.put("status", 200);
                entry.put("body", new String(body, StandardCharsets.UTF_8));
                entry.put("etag", "\"" + HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(body)) + "\"");
            } catch (CorruptFunctionVersionException e) {
                entry.put("status", 500);
                entry.put("error", "CORRUPT_ROW");
                entry.put("rowId", e.rowId());
            }
        }
        Files.writeString(Path.of(args[4]),
                Json.MAPPER.writerWithDefaultPrettyPrinter().writeValueAsString(out) + "\n", StandardCharsets.UTF_8);
    }
}
