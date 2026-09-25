package io.flowcatalyst.platform.function;

import io.flowcatalyst.platform.shared.json.Json;
import io.flowcatalyst.sdk.result.Result;
import io.flowcatalyst.sdk.usecase.UseCaseException;
import tools.jackson.databind.JsonNode;
import tools.jackson.databind.node.ArrayNode;
import tools.jackson.databind.node.ObjectNode;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Map;
import java.util.function.Function;

/// Golden-file generator for the function model (fc-function-model). Runs Java's REAL
/// `Manifest.check` / `parseStrict` / `readStored` / `toJson` and the value
/// types' parsers over every case in `tests/data/manifest-cases.json`
/// and writes what Java answered to `manifest-golden.json`. The Rust tests
/// feed the same inputs to the Rust port and require the same answers, and
/// the same normalised bytes.
///
/// Not part of any build. Regenerate from the pinned Java sources, compiled
/// outside the Java repo (javac compiles only the classes this reaches):
///
/// ```
/// git -C <javalin> archive 0118cdca | tar -x -C /tmp/pin
/// SP=/tmp/pin/server/src/main/java:/tmp/pin/usecase/src/main/java:/tmp/pin/sdk/src/main/java:/tmp/pin/function-api/src/main/java
/// CP=<jackson-databind-3.1.5>:<jackson-core-3.1.5>:<jackson-annotations-2.22>:<slf4j-api>
/// javac -proc:none -d /tmp/golden -cp $CP -sourcepath $SP \
///     crates/fc-function-model/tests/java/io/flowcatalyst/platform/function/ManifestGoldenGen.java
/// java -cp /tmp/golden:$CP io.flowcatalyst.platform.function.ManifestGoldenGen \
///     crates/fc-function-model/tests/data/manifest-cases.json \
///     crates/fc-function-model/tests/data/manifest-golden.json
/// ```
public final class ManifestGoldenGen {

    public static void main(String[] args) throws Exception {
        JsonNode doc = Json.MAPPER.readTree(Files.readString(Path.of(args[0]), StandardCharsets.UTF_8));
        ObjectNode out = Json.MAPPER.createObjectNode();

        ArrayNode manifests = out.putArray("manifest");
        for (JsonNode c : doc.path("manifest")) manifests.add(manifestCase(c));

        ArrayNode stored = out.putArray("stored");
        for (JsonNode c : doc.path("stored")) stored.add(storedCase(c));

        ObjectNode values = out.putObject("values");
        JsonNode v = doc.path("values");
        values.set("dnsLabel", table(v.path("dnsLabel"), raw -> DnsLabel.parse("field", raw).value()));
        values.set("hostname", table(v.path("hostname"),
                raw -> Hostname.parse(raw).value() + " " + String.join(",", Hostname.parse(raw).zoneCandidates())));
        values.set("digest", table(v.path("digest"), raw -> Digest.parse(raw).value()));
        values.set("settingKey", table(v.path("settingKey"), raw -> SettingKey.parse(raw).value()));
        values.set("routePattern", table(v.path("routePattern"), raw -> describe(RoutePattern.parse(raw))));
        values.set("addressPattern", table(v.path("addressPattern"), raw -> {
            FunctionAddressPattern p = FunctionAddressPattern.parse(raw);
            return p.getClass().getSimpleName() + " " + p.render();
        }));
        values.set("poolUrl", table(v.path("poolUrl"), raw -> {
            PoolUrlTemplate t = PoolUrlTemplate.parse(raw);
            return t.template() + " " + t.resolve(new DnsLabel("orders"));
        }));
        values.set("runtime", table(v.path("runtime"), raw -> Runtime.parseStrict(raw).name()));
        values.set("httpMethod", table(v.path("httpMethod"), raw -> HttpMethod.parseStrict(raw).name()));
        values.set("endpointAuth", table(v.path("endpointAuth"), raw -> EndpointAuth.parseStrict(raw).name()));

        ArrayNode matches = out.putArray("matches");
        for (JsonNode m : doc.path("matches")) {
            ObjectNode row = matches.addObject();
            row.put("pattern", m.get(0).asString());
            row.put("path", m.get(1).asString());
            var result = RoutePattern.parse(m.get(0).asString()).match(m.get(1).asString());
            if (result.isPresent()) {
                ObjectNode params = row.putObject("params");
                for (Map.Entry<String, String> e : result.get().entrySet()) params.put(e.getKey(), e.getValue());
            } else {
                row.putNull("params");
            }
        }

        Files.writeString(Path.of(args[1]),
                Json.MAPPER.writerWithDefaultPrettyPrinter().writeValueAsString(out) + "\n",
                StandardCharsets.UTF_8);
    }

    private static ClientCeilings ceilings(JsonNode c, FunctionLimits defaults) {
        JsonNode ce = c.path("ceilings");
        if (!ce.isArray()) return ClientCeilings.of(defaults);
        return new ClientCeilings(ce.get(0).asInt(), ce.get(1).asInt(), ce.get(2).asInt(), ce.get(3).asInt());
    }

    private static JsonNode root(JsonNode c) {
        JsonNode text = c.path("text");
        return text.isString() ? Json.MAPPER.readTree(text.asString()) : null;
    }

    private static ObjectNode manifestCase(JsonNode c) {
        FunctionLimits defaults = FunctionLimits.defaults();
        ClientCeilings ceilings = ceilings(c, defaults);
        Runtime runtime = Runtime.parse(c.path("runtime").asString());
        ObjectNode row = Json.MAPPER.createObjectNode();
        row.put("name", c.path("name").asString());
        JsonNode root = root(c);
        try {
            switch (Manifest.check(root, runtime, defaults, ceilings)) {
                case Result.Ok<Manifest, Manifest.ManifestRejected>(Manifest m) -> {
                    row.put("result", "ok");
                    row.put("normalised", Json.MAPPER.writeValueAsString(m.toJson()));
                    row.put("roundTrip", Manifest.readStored(m.toJson()).equals(m));
                }
                case Result.Err<Manifest, Manifest.ManifestRejected>(Manifest.ManifestRejected r) -> {
                    row.put("result", "rejected");
                    ArrayNode problems = row.putArray("problems");
                    for (Manifest.ManifestProblem p : r.problems()) {
                        ObjectNode problem = problems.addObject();
                        problem.put("code", p.code());
                        problem.put("message", p.message());
                        problem.put("pointer", p.pointer());
                    }
                    try {
                        Manifest.parseStrict(root, runtime, defaults, ceilings);
                        row.put("thrown", "NOTHING");
                    } catch (UseCaseException e) {
                        ObjectNode thrown = row.putObject("thrown");
                        thrown.put("code", e.code());
                        thrown.put("message", e.error().message());
                    }
                }
            }
        } catch (RuntimeException e) {
            row.put("result", "exception");
            row.put("exception", e.getClass().getSimpleName());
        }
        return row;
    }

    private static ObjectNode storedCase(JsonNode c) {
        ObjectNode row = Json.MAPPER.createObjectNode();
        row.put("name", c.path("name").asString());
        try {
            Manifest m = Manifest.readStored(root(c));
            row.put("result", "ok");
            row.put("normalised", Json.MAPPER.writeValueAsString(m.toJson()));
        } catch (IllegalStateException e) {
            row.put("result", "unreadable");
            row.put("message", e.getMessage());
        } catch (RuntimeException e) {
            row.put("result", "exception");
            row.put("exception", e.getClass().getSimpleName());
        }
        return row;
    }

    private static String describe(RoutePattern p) {
        StringBuilder sb = new StringBuilder(p.value()).append(' ');
        for (RoutePattern.Segment s : p.segments()) {
            switch (s) {
                case RoutePattern.Literal(String l) -> sb.append("L:").append(l).append('|');
                case RoutePattern.Param(String n) -> sb.append("P:").append(n).append('|');
                case RoutePattern.Rest r -> sb.append("R|");
            }
        }
        return sb.toString();
    }

    private static ArrayNode table(JsonNode inputs, Function<String, String> parse) {
        ArrayNode rows = Json.MAPPER.createArrayNode();
        for (JsonNode input : inputs) {
            ObjectNode row = rows.addObject();
            String raw = input.asString();
            row.put("input", raw);
            try {
                row.put("ok", parse.apply(raw));
            } catch (UseCaseException e) {
                row.put("code", e.code());
                row.put("message", e.error().message());
            } catch (IllegalStateException | IllegalArgumentException e) {
                row.put("code", e.getClass().getSimpleName());
                row.put("message", e.getMessage());
            }
        }
        return rows;
    }
}
