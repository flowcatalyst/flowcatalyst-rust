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
/// Not part of any build.
///
/// Provenance: generated from Java `0118cdca` (the classes on
/// `DesiredState.build`'s path that had changed after it recompiled from
/// the pin); regenerated from `45fd3444` (2026-09-30), every class as it is
/// there, with byte-identical output. Build Java from a read-only export,
/// never inside the Java repo.
///
/// The fixture writes Rust's `fnr_*` tables (owner decision #48); Java keeps
/// `fn_*`, so the generator loads a copy with the `INSERT INTO` targets
/// renamed back. Only the table names change: `'fnr_R1'` and the other
/// route ids are data and stay.
///
/// ```
/// J=<scratch>/javalin; mkdir -p $J
/// git -C ../flowcatalyst-javalin archive 45fd3444 | tar -x -C $J
/// (cd $J && mvn -B -DskipTests -pl server -am compile dependency:build-classpath -Dmdep.outputFile=cp.txt)
/// CP=$J/server/target/classes:$J/usecase/target/classes:$J/sdk/target/classes:$J/function-api/target/classes:$(cat $J/server/cp.txt)
/// javac -proc:none -d /tmp/golden -cp "$CP" \
///     crates/fc-platform/tests/java/io/flowcatalyst/platform/function/operations/DesiredStateGoldenGen.java
/// sed 's/^INSERT INTO fnr_/INSERT INTO fn_/' crates/fc-platform/tests/data/function/desired-state-fixture.sql \
///     > /tmp/desired-state-fixture-java.sql
/// docker run -d --rm -p 127.0.0.1:55987:5432 -e POSTGRES_PASSWORD=test -e POSTGRES_DB=golden postgres:16
/// java -cp "/tmp/golden:$CP" io.flowcatalyst.platform.function.operations.DesiredStateGoldenGen \
///     jdbc:postgresql://127.0.0.1:55987/golden postgres test \
///     /tmp/desired-state-fixture-java.sql \
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
