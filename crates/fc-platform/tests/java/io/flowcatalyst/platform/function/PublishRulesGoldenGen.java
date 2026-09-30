package io.flowcatalyst.platform.function;

import io.flowcatalyst.platform.function.artifact.PlatformArtifactRef;
import io.flowcatalyst.platform.function.artifact.SignaturesMode;
import io.flowcatalyst.platform.scheduledjob.cron.CronExpression;
import io.flowcatalyst.platform.shared.json.Json;
import io.flowcatalyst.sdk.usecase.UseCaseException;
import tools.jackson.databind.JsonNode;
import tools.jackson.databind.node.ObjectNode;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.ZoneId;

/// Golden-file generator for the rules a publish applies that Rust ports
/// outside the manifest: Java's cron grammar (`CronExpression.parse`, the
/// publish check's `CRON_INVALID` reason), `ZoneId.of` (`TIMEZONE_INVALID`),
/// `PlatformArtifactRef.parse` and `SignaturesMode.parse`. Runs Java's REAL
/// code over every input in `tests/data/function/publish-rules-cases.json`
/// and writes what Java answered to `publish-rules-golden.json`.
///
/// Not part of any build.
///
/// Provenance: generated from Java `0118cdca`; regenerated from `45fd3444`
/// (2026-09-30) with byte-identical output. Build Java from a read-only
/// export, never inside the Java repo:
///
/// ```
/// J=<scratch>/javalin; mkdir -p $J
/// git -C ../flowcatalyst-javalin archive 45fd3444 | tar -x -C $J
/// (cd $J && mvn -B -DskipTests -pl server -am compile dependency:build-classpath -Dmdep.outputFile=cp.txt)
/// CP=$J/server/target/classes:$J/usecase/target/classes:$J/sdk/target/classes:$J/function-api/target/classes:$(cat $J/server/cp.txt)
/// javac -proc:none -d /tmp/golden -cp "$CP" \
///     crates/fc-platform/tests/java/io/flowcatalyst/platform/function/PublishRulesGoldenGen.java
/// java -cp "/tmp/golden:$CP" io.flowcatalyst.platform.function.PublishRulesGoldenGen \
///     crates/fc-platform/tests/data/function/publish-rules-cases.json \
///     crates/fc-platform/tests/data/function/publish-rules-golden.json
/// ```
public final class PublishRulesGoldenGen {

    public static void main(String[] args) throws Exception {
        JsonNode doc = Json.MAPPER.readTree(Files.readString(Path.of(args[0]), StandardCharsets.UTF_8));
        ObjectNode out = Json.MAPPER.createObjectNode();

        ObjectNode cron = out.putObject("cron");
        for (JsonNode c : doc.path("cron")) {
            String text = c.asString();
            try {
                CronExpression.parse(text);
                cron.put(text, "OK");
            } catch (UseCaseException e) {
                cron.put(text, e.error().code() + " " + e.error().message());
            }
        }

        ObjectNode zones = out.putObject("zone");
        for (JsonNode z : doc.path("zone")) {
            String id = z.asString();
            boolean valid;
            try {
                ZoneId.of(id);
                valid = true;
            } catch (java.time.DateTimeException e) {
                valid = false;
            }
            zones.put(id, valid);
        }

        ObjectNode refs = out.putObject("platformRef");
        for (JsonNode r : doc.path("platformRef")) {
            String ref = r.asString();
            refs.put(ref, PlatformArtifactRef.parse(ref)
                    .map(p -> p.functionId() + " " + p.hex())
                    .orElse("EMPTY"));
        }

        ObjectNode modes = out.putObject("signaturesMode");
        for (JsonNode m : doc.path("signaturesMode")) {
            modes.put(m.asString(), SignaturesMode.parse(m.asString()).name());
        }

        Files.writeString(Path.of(args[1]), Json.MAPPER.writerWithDefaultPrettyPrinter().writeValueAsString(out) + "\n",
                StandardCharsets.UTF_8);
    }
}
