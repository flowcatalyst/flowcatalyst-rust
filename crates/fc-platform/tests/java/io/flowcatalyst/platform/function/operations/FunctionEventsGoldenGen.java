package io.flowcatalyst.platform.function.operations;

import io.flowcatalyst.platform.function.ClientCeilings;
import io.flowcatalyst.platform.function.ClientPolicy;
import io.flowcatalyst.platform.function.Digest;
import io.flowcatalyst.platform.function.DnsLabel;
import io.flowcatalyst.platform.function.Function;
import io.flowcatalyst.platform.function.FunctionAddress;
import io.flowcatalyst.platform.function.FunctionDomain;
import io.flowcatalyst.platform.function.FunctionLimits;
import io.flowcatalyst.platform.function.FunctionOwner;
import io.flowcatalyst.platform.function.FunctionStatus;
import io.flowcatalyst.platform.function.FunctionVersion;
import io.flowcatalyst.platform.function.Hostname;
import io.flowcatalyst.platform.function.Manifest;
import io.flowcatalyst.platform.function.Runtime;
import io.flowcatalyst.platform.function.SecretValue;
import io.flowcatalyst.platform.function.SignerIdentity;
import io.flowcatalyst.platform.shared.json.Json;
import io.flowcatalyst.sdk.usecase.DomainEvent;
import io.flowcatalyst.sdk.usecase.ExecutionContext;
import tools.jackson.databind.node.ArrayNode;
import tools.jackson.databind.node.ObjectNode;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Instant;
import java.util.List;
import java.util.Set;

/// Golden-file generator for fc-platform's function events. Runs Java's REAL
/// `FunctionEvents.*.of(...)` over fixed aggregates and writes, per event,
/// the envelope fields the Rust port must match (type, source, subject,
/// message group) and `data()` exactly as Java's `Json` mapper writes it
/// into `msg_events.data`. Also `SecretValue`'s JSON, the one form of a
/// secret a command's audit row can ever carry.
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
///     crates/fc-platform/tests/java/io/flowcatalyst/platform/function/operations/FunctionEventsGoldenGen.java
/// java -cp "/tmp/golden:$CP" io.flowcatalyst.platform.function.operations.FunctionEventsGoldenGen \
///     crates/fc-platform/tests/data/function/events-golden.json
/// ```
public final class FunctionEventsGoldenGen {

    private static final Instant T = Instant.parse("2026-09-24T10:00:00.123456Z");

    public static void main(String[] args) throws Exception {
        ExecutionContext ec = ExecutionContext.of("prn_1");
        FunctionAddress address = FunctionAddress.of(new DnsLabel("billing"), new DnsLabel("invoices"),
                new DnsLabel("create"));
        Function platformFn = new Function("fnc_1", "app_1", address, new FunctionOwner.Platform(), Runtime.WASM,
                null, FunctionStatus.ACTIVE, List.of(), T, T);
        Function clientFn = new Function("fnc_2", "app_1", address, FunctionOwner.ofClientId("clt_1"), Runtime.JVM,
                "Creates invoices", FunctionStatus.DISABLED, List.of(), T, T);
        ClientPolicy platformPolicy = new ClientPolicy(new FunctionOwner.Platform(),
                List.of(new ClientPolicy.SignerRule("https://issuer", "a", Set.of(Runtime.JVM)),
                        new ClientPolicy.SignerRule("https://issuer", "b", Set.of(Runtime.WASM))),
                9000, null, null, null, T, T);
        ClientPolicy clientPolicy = new ClientPolicy(FunctionOwner.ofClientId("clt_1"), List.of(),
                null, null, null, null, T, T);
        FunctionDomain platformDomain = new FunctionDomain("fnd_1", new FunctionOwner.Platform(),
                Hostname.parse("acme.com"), T);
        FunctionDomain clientDomain = new FunctionDomain("fnd_2", FunctionOwner.ofClientId("clt_1"),
                Hostname.parse("api.example.org"), T);

        ObjectNode out = Json.MAPPER.createObjectNode();
        ArrayNode events = out.putArray("events");
        add(events, "created/platform", FunctionEvents.FunctionCreated.of(ec, platformFn));
        add(events, "created/client", FunctionEvents.FunctionCreated.of(ec, clientFn));
        add(events, "updated/platform", FunctionEvents.FunctionUpdated.of(ec, platformFn));
        add(events, "updated/client", FunctionEvents.FunctionUpdated.of(ec, clientFn));
        add(events, "deleted/client", FunctionEvents.FunctionDeleted.of(ec, clientFn));
        add(events, "config/client", FunctionEvents.ConfigUpdated.of(ec, clientFn, List.of("A", "B")));
        add(events, "config/empty", FunctionEvents.ConfigUpdated.of(ec, clientFn, List.of()));
        add(events, "secret-set/client", FunctionEvents.SecretSet.of(ec, clientFn, "API_KEY"));
        add(events, "secret-deleted/client", FunctionEvents.SecretDeleted.of(ec, clientFn, "API_KEY"));
        add(events, "policy/platform", FunctionEvents.PolicyUpdated.of(ec, platformPolicy));
        add(events, "policy/client", FunctionEvents.PolicyUpdated.of(ec, clientPolicy));
        add(events, "domain-claimed/platform", FunctionEvents.DomainClaimed.of(ec, platformDomain));
        add(events, "domain-claimed/client", FunctionEvents.DomainClaimed.of(ec, clientDomain));
        add(events, "domain-released/client", FunctionEvents.DomainReleased.of(ec, clientDomain));

        FunctionLimits defaults = FunctionLimits.defaults();
        Manifest manifest = Manifest.parseStrict(
                Json.MAPPER.readTree("{\"runtime\":\"wasm\",\"entrypoint\":\"handle\",\"pool\":\"edge\"}"),
                Runtime.WASM, defaults, ClientCeilings.of(defaults));
        Digest digest = Digest.parse("sha256:" + "b".repeat(64));
        FunctionVersion signed = new FunctionVersion("fnv_3", "fnc_1", 3, "platform://fnc_1/abc", digest, "{}", null,
                new SignerIdentity("https://issuer", "repo:acme/fn"), manifest,
                new FunctionVersion.VersionState.Published(), "prn_1", T);
        FunctionVersion unsigned = new FunctionVersion("fnv_4", "fnc_2", 4, "oci://r/a", digest, null, null, null,
                manifest, new FunctionVersion.VersionState.Published(), "prn_1", T);
        add(events, "version-published/signed", FunctionEvents.VersionPublished.of(ec, platformFn, signed));
        add(events, "version-published/unsigned", FunctionEvents.VersionPublished.of(ec, clientFn, unsigned));
        add(events, "version-retired/client", FunctionEvents.VersionRetired.of(ec, clientFn, unsigned));

        out.put("secretValue", Json.write(new SecretValue("sk_live_MARKER")));

        Files.writeString(Path.of(args[0]), Json.MAPPER.writerWithDefaultPrettyPrinter().writeValueAsString(out) + "\n",
                StandardCharsets.UTF_8);
    }

    private static void add(ArrayNode events, String name, DomainEvent event) {
        ObjectNode e = events.addObject();
        e.put("case", name);
        e.put("type", event.metadata().type());
        e.put("source", event.metadata().source());
        e.put("specVersion", event.metadata().specVersion());
        e.put("subject", event.metadata().subject());
        e.put("messageGroup", event.metadata().messageGroup());
        e.put("data", Json.write(event.data()));
    }
}
