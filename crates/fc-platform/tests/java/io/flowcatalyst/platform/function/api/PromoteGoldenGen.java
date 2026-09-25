package io.flowcatalyst.platform.function.api;

import io.flowcatalyst.platform.function.operations.FunctionTriggerSync;
import io.flowcatalyst.platform.function.operations.PromotePlan;
import io.flowcatalyst.platform.scheduledjob.cron.CronExpression;
import io.flowcatalyst.platform.shared.json.Json;
import io.flowcatalyst.router.wire.WebhookSigner;
import tools.jackson.databind.node.ArrayNode;
import tools.jackson.databind.node.ObjectNode;

import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Instant;
import java.time.ZoneId;
import java.time.ZonedDateTime;
import java.util.List;

/// Golden-file generator for promote wiring (P5). Runs Java's REAL code and
/// writes what it answered to `tests/data/function/promote-golden.json`:
///
/// - `keys`: `FunctionTriggerSync`'s private `fid` and `hash8` (by
///   reflection), and the trigger keys it builds from them: `fn-<fid>` for
///   the pool, `fn-<fid>-<hash8(eventType)>`, `fn-<fid>-<hash8(cron NUL zone)>`.
/// - `cron`: `CronExpression.next`, the scheduler's reader, walked eight
///   steps from a fixed instant in a zone, for crons a manifest may declare.
/// - `plans`: `FunctionApi.PromotePlanResponse.from(plan)` written by Java's
///   `Json` mapper: the `plan` of `POST …/manifest/check`, byte for byte.
/// - `signatures`: `WebhookSigner.timestamp` and `WebhookSigner.sign`.
///
/// Not part of any build. `FunctionTriggerSync` and `FunctionApi` reach
/// most of the server, so this runs against the Java repo's own compiled
/// classes rather than a `-sourcepath` compile. Every class it exercises
/// (`FunctionTriggerSync`, `PromotePlan`, `FunctionApi`, `CronExpression`,
/// `Json`, `WebhookSigner`) is byte-identical between the pin `0118cdca` and
/// the checkout those classes were built from (`git diff --quiet 0118cdca
/// HEAD -- <file>` for each):
///
/// ```
/// J=../flowcatalyst-javalin; M=~/.m2/repository
/// CP=$J/server/target/classes:$M/tools/jackson/core/jackson-databind/3.1.5/jackson-databind-3.1.5.jar:$M/tools/jackson/core/jackson-core/3.1.5/jackson-core-3.1.5.jar:$M/com/fasterxml/jackson/core/jackson-annotations/2.22/jackson-annotations-2.22.jar:$M/org/slf4j/slf4j-api/2.0.9/slf4j-api-2.0.9.jar
/// javac -proc:none -d /tmp/golden -cp $CP \
///     crates/fc-platform/tests/java/io/flowcatalyst/platform/function/api/PromoteGoldenGen.java
/// java -cp /tmp/golden:$CP io.flowcatalyst.platform.function.api.PromoteGoldenGen \
///     crates/fc-platform/tests/data/function/promote-golden.json
/// ```
public final class PromoteGoldenGen {

    private static final List<String> FUNCTION_IDS = List.of("fnc_0HZXEQ5Y8JY5Z", "fnc_ABCDEFGHJKMNP");

    private static final List<String> EVENT_TYPES = List.of(
            "orders:fulfillment:shipment:shipped", "billing:invoice:x:created", "a:b:c:d", "ü:é:ñ:ø");

    private static final List<List<String>> SCHEDULES = List.of(
            List.of("0 0 * * * *", ""), List.of("0 0 * * * *", "Europe/Amsterdam"),
            List.of("0 30 9 * * mon-fri", "America/New_York"), List.of("*/15 * * * * *", ""));

    /// Crons Java accepts, each walked in every zone from START: plain
    /// fields, days of the week by number and by name (0 is Sunday), the
    /// either-day rule when both day fields are restricted, `N/S`, signs,
    /// month names, and zones that are regions and fixed offsets.
    private static final List<String> CRONS = List.of(
            "0 0 * * * *", "*/15 * * * * *", "0 */5 9-17 * * *", "0 0 9 * * 1-5", "0 30 9 * * mon-fri",
            "0 0 12 ? * 0", "0 0 12 ? * SUN,sat", "0 0 0 13 * 5", "0 30 8 1-7 * mon", "0 0 12 1-31 * mon",
            "0 0 0 */2 * 1", "5/20 0 0 * * *", "+5 0 0 * * *", "0 0 0 1 JAN,jul *", "0 0 0 1,15 * ?",
            "0 0 2 * * *", "0 0 0 29 2 *", "0 0 0 31 * *", "30 45 23 * * 6", "0 0 */6 * * *");

    private static final List<String> ZONES = List.of("UTC", "America/New_York", "Asia/Kolkata", "+05:30",
            "UTC-03:00", "Z", "Europe/Amsterdam");

    /// The day before US daylight saving starts, so hourly walks cross the gap.
    private static final Instant START = Instant.parse("2026-03-07T12:00:00Z");

    private static final int STEPS = 8;

    public static void main(String[] args) throws Exception {
        ObjectNode out = Json.MAPPER.createObjectNode();

        Method fid = FunctionTriggerSync.class.getDeclaredMethod("fid", String.class);
        fid.setAccessible(true);
        Method hash8 = FunctionTriggerSync.class.getDeclaredMethod("hash8", String.class);
        hash8.setAccessible(true);

        ArrayNode keys = out.putArray("keys");
        for (String functionId : FUNCTION_IDS) {
            String f = (String) fid.invoke(null, functionId);
            ObjectNode k = keys.addObject();
            k.put("functionId", functionId);
            k.put("fid", f);
            k.put("pool", "fn-" + f);
            ObjectNode subs = k.putObject("subscriptions");
            for (String et : EVENT_TYPES) {
                subs.put(et, "fn-" + f + "-" + hash8.invoke(null, et));
            }
            ArrayNode schedules = k.putArray("schedules");
            for (List<String> s : SCHEDULES) {
                ObjectNode e = schedules.addObject();
                e.put("cron", s.get(0));
                if (!s.get(1).isEmpty()) e.put("timezone", s.get(1));
                e.put("key", "fn-" + f + "-" + hash8.invoke(null, s.get(0) + "\0" + s.get(1)));
            }
        }

        out.put("start", START.toString());
        ArrayNode cron = out.putArray("cron");
        for (String text : CRONS) {
            CronExpression expression = CronExpression.parse(text);
            for (String zone : ZONES) {
                ObjectNode c = cron.addObject();
                c.put("cron", text);
                c.put("zone", zone);
                ArrayNode fires = c.putArray("fires");
                ZonedDateTime t = START.atZone(ZoneId.of(zone));
                for (int i = 0; i < STEPS; i++) {
                    var next = expression.next(t);
                    if (next.isEmpty()) break;
                    t = next.get();
                    fires.add(t.toInstant().toString());
                }
            }
        }

        ObjectNode plans = out.putObject("plans");
        plans.put("live", json(new PromotePlan("live", null, 1, List.of("GREETING", "API_KEY"),
                new PromotePlan.Wiring.Live(new PromotePlan.PoolAction.Create("fn-abc"),
                        List.of(new PromotePlan.SubscriptionAction.Create("fn-abc-11111111", "a:b:c:created"),
                                new PromotePlan.SubscriptionAction.Update("fn-abc-22222222", "a:b:c:updated",
                                        List.of("subscription")),
                                new PromotePlan.SubscriptionAction.Unchanged("fn-abc-33333333", "a:b:c:kept"),
                                new PromotePlan.SubscriptionAction.Delete("fn-abc-44444444", "a:b:c:dropped")),
                        List.of(new PromotePlan.ScheduleAction.Create("fn-abc-55555555", "0 0 * * * *", null),
                                new PromotePlan.ScheduleAction.Update("fn-abc-66666666", "0 0 9 * * 1-5",
                                        "Europe/Amsterdam", List.of("definition")),
                                new PromotePlan.ScheduleAction.Unchanged("fn-abc-77777777", "0 0 1 * * *", null),
                                new PromotePlan.ScheduleAction.Delete("fn-abc-88888888", "0 0 2 * * *", "UTC")),
                        new PromotePlan.PublicRoutesAction.Replace(
                                List.of(new PromotePlan.RouteKey("api.acme.com", "/", List.of()),
                                        new PromotePlan.RouteKey("acme.com", "/v2", List.of("qa", "dev"))),
                                List.of(new PromotePlan.RouteKey("old.acme.com", "/api", List.of("qa"))))),
                List.of(new PromotePlan.Conflict("PUBLIC_ROUTE_TAKEN", "route 'api.acme.com/' is already taken")))));
        plans.put("liveUnchanged", json(new PromotePlan("live", 3, 4, List.of(),
                new PromotePlan.Wiring.Live(new PromotePlan.PoolAction.Update("fn-abc", List.of("maxConcurrency")),
                        List.of(), List.of(), new PromotePlan.PublicRoutesAction.Unchanged()),
                List.of())));
        plans.put("poolUnchanged", json(new PromotePlan("live", 4, 5, List.of(),
                new PromotePlan.Wiring.Live(new PromotePlan.PoolAction.Unchanged("fn-abc"),
                        List.of(), List.of(), new PromotePlan.PublicRoutesAction.Unchanged()),
                List.of(new PromotePlan.Conflict("TRIGGER_KEY_COLLISION", "two subscriptions entries collide")))));
        plans.put("named", json(new PromotePlan("qa", 2, 3, List.of("GREETING"), new PromotePlan.Wiring.HttpOnly(),
                List.of())));
        plans.put("namedUnset", json(new PromotePlan("qa", null, 1, List.of(), new PromotePlan.Wiring.HttpOnly(),
                List.of())));

        ArrayNode signatures = out.putArray("signatures");
        for (String[] s : new String[][] {
                {"secret", "2026-08-07T09:32:12.123Z", "hello"},
                {"whsec_function", "2026-09-25T10:00:00Z", "{\"jobCode\":\"fn-abc-12345678\"}"},
                {"ü-secret", "2026-01-01T00:00:00.000999Z", ""}}) {
            ObjectNode e = signatures.addObject();
            e.put("secret", s[0]);
            e.put("at", s[1]);
            e.put("body", s[2]);
            String timestamp = WebhookSigner.timestamp(Instant.parse(s[1]));
            e.put("timestamp", timestamp);
            e.put("signature", WebhookSigner.sign(s[0], timestamp, s[2].getBytes(StandardCharsets.UTF_8)));
        }

        Files.writeString(Path.of(args[0]), Json.MAPPER.writerWithDefaultPrettyPrinter().writeValueAsString(out) + "\n",
                StandardCharsets.UTF_8);
    }

    private static String json(PromotePlan plan) {
        return Json.write(FunctionApi.PromotePlanResponse.from(plan));
    }
}
