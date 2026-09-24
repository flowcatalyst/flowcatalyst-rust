package io.flowcatalyst.fnhost.wasm;

import io.flowcatalyst.function.Caller;
import io.flowcatalyst.function.EventEmitException;
import io.flowcatalyst.function.Events;
import io.flowcatalyst.function.FunctionAddress;
import io.flowcatalyst.function.OutboundEvent;
import io.flowcatalyst.function.Request;
import io.flowcatalyst.function.WebhookGolden;

import java.io.IOException;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Base64;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;

/// Golden-file generator for `fc-function-abi`. Runs Java's REAL encoder and
/// decoders (`WasmAbi.encode` / `WasmAbi.decode`, `HostFunctions.emit`, and
/// `Webhook` / `Result` via [WebhookGolden]) and writes their exact output
/// bytes under `tests/data/java-golden/`. The Rust tests then compare their
/// own output with these bytes, so byte-identity is proven against Java
/// itself rather than against a re-reading of its source.
///
/// Not part of any build. Regenerate (from the Java repo root, after
/// `mvn -q -pl function-host -am package -DskipTests`):
///
/// ```
/// CP=function-host/target/classes:function-api/target/classes:function-host/target/flowcatalyst-function-host-0.0.1-SNAPSHOT-exec.jar
/// javac -d /tmp/golden -cp $CP <rust-repo>/crates/fc-function-abi/tests/java/io/flowcatalyst/**/*.java
/// java -cp /tmp/golden:$CP io.flowcatalyst.fnhost.wasm.GoldenGen <rust-repo>/crates/fc-function-abi/tests/data/java-golden
/// ```
public final class GoldenGen {

    private static final Base64.Encoder B64 = Base64.getEncoder();

    public static void main(String[] args) throws Exception {
        Path out = Path.of(args[0]);
        Files.createDirectories(out.resolve("encode"));
        encodeFixtures(out.resolve("encode"));
        decodeTable(out.resolve("decode.tsv"));
        emitTable(out.resolve("emit.tsv"));
        WebhookGolden.write(out);
    }

    // ── encode ──────────────────────────────────────────────────────────────

    private static Request minimal(Caller caller) {
        return new Request(new FunctionAddress("a", "s", "n"), 3, "inv", "GET", "/p", null, null,
                Map.of(), Map.of(), Map.of(), new byte[0], null, caller);
    }

    private static void encodeFixtures(Path dir) throws IOException {
        Files.write(dir.resolve("platform-minimal.json"), WasmAbi.encode(minimal(Caller.Platform.INSTANCE)));
        Files.write(dir.resolve("anonymous-minimal.json"), WasmAbi.encode(minimal(Caller.Anonymous.INSTANCE)));
        Files.write(dir.resolve("principal-wasmabitest.json"), WasmAbi.encode(minimal(
                new Caller.Principal("prn_1", "SERVICE", "CLIENT", List.of("clt_1", "clt_2"), List.of("role-a"),
                        List.of("app_1"), false, Set.of("b:perm", "a:perm")))));

        Map<String, String> pathParams = new LinkedHashMap<>();
        pathParams.put("orderId", "42");
        pathParams.put("z", "last");
        pathParams.put("a", "first");
        Map<String, List<String>> query = new LinkedHashMap<>();
        query.put("q", List.of("a b", "c"));
        query.put("empty", List.of());
        query.put("dup", List.of("1", "1"));
        Map<String, List<String>> headers = new LinkedHashMap<>();
        headers.put("Content-Type", List.of("application/json"));
        headers.put("X-Multi", List.of("1", "2"));
        headers.put("accept", List.of("*/*"));
        Set<String> perms = new LinkedHashSet<>(List.of(
                "platform:*:event-type:view", "a:b:c:d", "*:*:*:*", "Z:upper", "\u00e9clair:x"));
        Files.write(dir.resolve("full.json"), WasmAbi.encode(new Request(
                FunctionAddress.parse("billing.invoices.api"), 12, "0HZXEQ5Y8JY5Z", "POST", "/orders/42",
                "api.acme.com", "/v1/orders/42", pathParams, query, headers,
                new byte[] {0x00, (byte) 0xff, 0x10, 'h', 'i'}, "10.0.0.1",
                new Caller.Principal("prn_0HZ", "service-account", null, List.of("clt_1"), List.of(),
                        List.of("app_1", "app_2"), true, perms))));

        Map<String, String> escParams = new LinkedHashMap<>();
        escParams.put("p", "\u00a0");
        Map<String, List<String>> escQuery = new LinkedHashMap<>();
        escQuery.put("k/ey", List.of("/"));
        Map<String, List<String>> escHeaders = new LinkedHashMap<>();
        escHeaders.put("X-Ctl\u0000", List.of("v\u001b"));
        Files.write(dir.resolve("escaping.json"), WasmAbi.encode(new Request(
                FunctionAddress.parse("b-1.inv-2.c-3"), 2147483647, "", "",
                "/a\"b\\c/d\u0001\u001f\b\t\n\f\r\u007f \u00e9\uD83D\uDE00\u2028\u2029<>&'",
                "h\u00e9llo.example", "", escParams, escQuery, escHeaders, new byte[] {0x41}, "::1",
                Caller.Anonymous.INSTANCE)));

        Set<String> sortPerms = new LinkedHashSet<>(List.of(
                "b", "a", "B", "\u00e9", "\uD83D\uDE00", "\uE000", "\uFFFD", "a:b", "a"));
        Files.write(dir.resolve("principal-sorting.json"), WasmAbi.encode(new Request(
                new FunctionAddress("a", "s", "n"), 0, "inv", "DELETE", "/", null, "/x", Map.of(), Map.of(),
                Map.of(), new byte[] {1, 2}, "127.0.0.1",
                new Caller.Principal("id\"1", "user", "ANCHOR", List.of("*"), List.of("r\"1", "r/2"), List.of(),
                        false, sortPerms))));
    }

    // ── decode ──────────────────────────────────────────────────────────────

    private record Row(String name, byte[] input) {
    }

    private static Row row(String name, String input) {
        return new Row(name, input.getBytes(StandardCharsets.UTF_8));
    }

    private static String nested(int depth) {
        return "{\"status\":200,\"x\":" + "[".repeat(depth) + "0" + "]".repeat(depth) + "}";
    }

    private static void decodeTable(Path file) throws IOException {
        List<Row> rows = new ArrayList<>();
        // WasmAbiTest accepted rows
        rows.add(row("text body", "{\"status\":201,\"body\":\"h\u00e9llo\"}"));
        rows.add(row("base64 body", "{\"status\":200,\"bodyBase64\":\"aGk=\"}"));
        rows.add(row("no body at all", "{\"status\":204}"));
        rows.add(row("headers of string arrays", "{\"status\":200,\"headers\":{\"a\":[\"1\",\"2\"]},\"body\":\"x\"}"));
        rows.add(row("unknown keys ignored", "{\"status\":200,\"body\":\"x\",\"extra\":true}"));
        rows.add(row("null body is no body", "{\"status\":200,\"body\":null}"));
        // WasmAbiTest malformed rows
        rows.add(row("not JSON", "this is not the result shape"));
        rows.add(row("not an object", "[200]"));
        rows.add(row("no status", "{\"body\":\"x\"}"));
        rows.add(row("status not an integer", "{\"status\":\"200\"}"));
        rows.add(row("status below range", "{\"status\":99}"));
        rows.add(row("status above range", "{\"status\":600}"));
        rows.add(row("both bodies", "{\"status\":200,\"body\":\"x\",\"bodyBase64\":\"eA==\"}"));
        rows.add(row("body not a string", "{\"status\":200,\"body\":{\"a\":1}}"));
        rows.add(row("bodyBase64 not base64", "{\"status\":200,\"bodyBase64\":\"!!!\"}"));
        rows.add(row("headers not an object", "{\"status\":200,\"headers\":[\"a\"]}"));
        rows.add(row("header value not an array", "{\"status\":200,\"headers\":{\"a\":\"1\"}}"));
        rows.add(row("header array of non-strings", "{\"status\":200,\"headers\":{\"a\":[1]}}"));
        // extra: document-level edges
        rows.add(row("x: empty input", ""));
        rows.add(row("x: whitespace only", "   "));
        rows.add(row("x: literal null", "null"));
        rows.add(row("x: trailing whitespace", "{\"status\":200} \n"));
        rows.add(row("x: trailing garbage", "{\"status\":200} x"));
        rows.add(row("x: second document", "{\"status\":200}{}"));
        rows.add(row("x: trailing bracket", "{\"status\":200}]"));
        rows.add(row("x: utf-8 bom", "\uFEFF{\"status\":200}"));
        rows.add(row("x: trailing comma", "{\"status\":200,}"));
        rows.add(row("x: comment", "{\"status\":200 /* c */}"));
        rows.add(row("x: single quotes", "{'status':200}"));
        rows.add(row("x: unquoted key", "{status:200}"));
        rows.add(row("x: NaN", "{\"status\":200,\"x\":NaN}"));
        rows.add(row("x: raw control char in string", "{\"status\":200,\"body\":\"a\u0001b\"}"));
        rows.add(new Row("x: invalid utf-8 in string", concat("{\"status\":200,\"body\":\"a", new byte[] {(byte) 0xff}, "\"}")));
        rows.add(new Row("x: invalid utf-8 outside string", concat("{\"status\":200}", new byte[] {(byte) 0xc3}, "")));
        // status edges
        rows.add(row("x: status float", "{\"status\":200.0}"));
        rows.add(row("x: status exponent", "{\"status\":2e2}"));
        rows.add(row("x: status negative zero", "{\"status\":-0}"));
        rows.add(row("x: status leading zero", "{\"status\":0200}"));
        rows.add(row("x: status plus sign", "{\"status\":+200}"));
        rows.add(row("x: status boundary 100", "{\"status\":100}"));
        rows.add(row("x: status boundary 599", "{\"status\":599}"));
        rows.add(row("x: status long", "{\"status\":2147483848}"));
        rows.add(row("x: status null", "{\"status\":null}"));
        rows.add(row("x: status bool", "{\"status\":true}"));
        rows.add(row("x: duplicate status last wins", "{\"status\":200,\"status\":201}"));
        rows.add(row("x: duplicate status to invalid", "{\"status\":200,\"status\":99}"));
        // headers edges
        rows.add(row("x: headers null", "{\"status\":200,\"headers\":null}"));
        rows.add(row("x: headers empty", "{\"status\":200,\"headers\":{}}"));
        rows.add(row("x: header empty array", "{\"status\":200,\"headers\":{\"a\":[]}}"));
        rows.add(row("x: duplicate header key", "{\"status\":200,\"headers\":{\"a\":[\"1\"],\"b\":[\"x\"],\"a\":[\"y\"]}}"));
        rows.add(row("x: header order kept", "{\"status\":200,\"headers\":{\"z\":[\"1\"],\"A\":[\"2\"],\"m\":[\"3\",\"4\"]}}"));
        rows.add(row("x: header null value", "{\"status\":200,\"headers\":{\"a\":null}}"));
        rows.add(row("x: header null element", "{\"status\":200,\"headers\":{\"a\":[null]}}"));
        rows.add(row("x: header nested array", "{\"status\":200,\"headers\":{\"a\":[[\"1\"]]}}"));
        rows.add(row("x: header lone surrogate", "{\"status\":200,\"headers\":{\"a\":[\"\\udc00\"]}}"));
        rows.add(row("x: headers string", "{\"status\":200,\"headers\":\"a\"}"));
        // body edges
        rows.add(row("x: text body with base64 null", "{\"status\":200,\"body\":\"x\",\"bodyBase64\":null}"));
        rows.add(row("x: both null", "{\"status\":200,\"body\":null,\"bodyBase64\":null}"));
        rows.add(row("x: body number", "{\"status\":200,\"body\":5}"));
        rows.add(row("x: body bool", "{\"status\":200,\"body\":false}"));
        rows.add(row("x: body array", "{\"status\":200,\"body\":[\"x\"]}"));
        rows.add(row("x: bodyBase64 number", "{\"status\":200,\"bodyBase64\":5}"));
        rows.add(row("x: body empty string", "{\"status\":200,\"body\":\"\"}"));
        rows.add(row("x: body escapes", "{\"status\":200,\"body\":\"a\\u0000b\\n\\\"\\/\\u00e9\\ud83d\\ude00\"}"));
        rows.add(row("x: body lone high surrogate", "{\"status\":200,\"body\":\"a\\ud800b\"}"));
        rows.add(row("x: body lone low surrogate", "{\"status\":200,\"body\":\"\\udc00\"}"));
        rows.add(row("x: body reversed surrogates", "{\"status\":200,\"body\":\"\\ude00\\ud83d\"}"));
        rows.add(row("x: body raw non-bmp", "{\"status\":200,\"body\":\"\uD83D\uDE00\"}"));
        rows.add(row("x: body bad escape", "{\"status\":200,\"body\":\"\\x\"}"));
        rows.add(row("x: body short unicode escape", "{\"status\":200,\"body\":\"\\u12\"}"));
        for (String b64 : List.of("", "aGk", "aGl=", "aGk==", "aG=k", "a", "=", "==", "aGk=\n", "aG k=", "-_8=", "+/8=",
                "aGVsbG8=", "aGVsbG8", "aGVsbG", "YQ", "YR==", "YQ=", "YQ===", "YWI=", "YWJ=", "YWJj", "YWJj=",
                "YWJj====", "Y", "YW", "YWJjZA", "YWJjZA=", "YWJjZA==", "\\u0061Gk=", "aGk=aGk=", "AAAA", "////")) {
            rows.add(row("b64: \"" + b64.replace("\n", "\\n") + "\"",
                    "{\"status\":200,\"bodyBase64\":\"" + b64.replace("\n", "\\n") + "\"}"));
        }
        // nesting and size limits in ignored members
        for (int depth : List.of(64, 128, 129, 499, 500, 501, 999, 1000, 1001)) {
            rows.add(row("x: nesting " + depth, nested(depth)));
        }
        for (int digits : List.of(999, 1000, 1001)) {
            rows.add(row("x: number digits " + digits, "{\"status\":200,\"x\":" + "1".repeat(digits) + "}"));
        }
        rows.add(row("x: float length 1001 (int part)", "{\"status\":200,\"x\":" + "1".repeat(999) + ".5}"));
        rows.add(row("x: float length 1000 (int part)", "{\"status\":200,\"x\":" + "1".repeat(998) + ".5}"));
        rows.add(row("x: float fraction 1000", "{\"status\":200,\"x\":0." + "1".repeat(1000) + "}"));
        rows.add(row("x: float fraction 1001", "{\"status\":200,\"x\":0." + "1".repeat(1001) + "}"));
        rows.add(row("x: float fraction 998", "{\"status\":200,\"x\":0." + "1".repeat(998) + "}"));
        rows.add(row("x: float fraction 999", "{\"status\":200,\"x\":0." + "1".repeat(999) + "}"));
        rows.add(row("x: exponent digits 1000", "{\"status\":200,\"x\":1e" + "0".repeat(999) + "1}"));
        rows.add(row("x: exponent digits 1001", "{\"status\":200,\"x\":1e" + "0".repeat(1000) + "1}"));
        rows.add(row("x: negative int digits 1000", "{\"status\":200,\"x\":-" + "1".repeat(1000) + "}"));
        rows.add(row("x: negative int digits 1001", "{\"status\":200,\"x\":-" + "1".repeat(1001) + "}"));
        rows.add(row("x: name length 50000", "{\"status\":200,\"" + "k".repeat(50000) + "\":1}"));
        rows.add(row("x: name length 50001", "{\"status\":200,\"" + "k".repeat(50001) + "\":1}"));
        rows.add(row("x: header name length 50001", "{\"status\":200,\"headers\":{\"" + "k".repeat(50001) + "\":[]}}"));
        rows.add(new Row("x: overlong utf-8 in string", concat("{\"status\":200,\"body\":\"a", new byte[] {(byte) 0xc0, (byte) 0xaf}, "\"}")));
        rows.add(new Row("x: surrogate utf-8 in string", concat("{\"status\":200,\"body\":\"a", new byte[] {(byte) 0xed, (byte) 0xa0, (byte) 0x80}, "\"}")));
        rows.add(new Row("x: 4-byte utf-8 in string", concat("{\"status\":200,\"body\":\"a", new byte[] {(byte) 0xf0, (byte) 0x9f, (byte) 0x98, (byte) 0x80}, "\"}")));
        rows.add(new Row("x: F5 utf-8 in string", concat("{\"status\":200,\"body\":\"a", new byte[] {(byte) 0xf5, (byte) 0x80, (byte) 0x80, (byte) 0x80}, "\"}")));
        rows.add(new Row("x: truncated utf-8 in string", concat("{\"status\":200,\"body\":\"a", new byte[] {(byte) 0xe2, (byte) 0x82}, "\"}")));
        rows.add(new Row("x: lone continuation in string", concat("{\"status\":200,\"body\":\"a", new byte[] {(byte) 0x80}, "\"}")));
        rows.add(new Row("x: utf-8 in key", concat("{\"status\":200,\"headers\":{\"", new byte[] {(byte) 0xc3, (byte) 0xa9}, "\":[\"1\"]}}")));
        rows.add(new Row("x: overlong utf-8 in key", concat("{\"status\":200,\"headers\":{\"", new byte[] {(byte) 0xc0, (byte) 0xaf}, "\":[\"1\"]}}")));
        rows.add(new Row("x: bad utf-8 in key", concat("{\"status\":200,\"headers\":{\"", new byte[] {(byte) 0xff}, "\":[\"1\"]}}")));
        rows.add(row("x: uppercase unicode escape", "{\"status\":200,\"body\":\"\\u00E9\\u00e9\"}"));
        rows.add(row("x: escaped key", "{\"stat\\u0075s\":200}"));
        rows.add(row("x: tab whitespace", "\t{\r\n\"status\" :\t200 }"));
        rows.add(row("x: form feed whitespace", "\f{\"status\":200}"));
        rows.add(row("x: nbsp whitespace", "\u00a0{\"status\":200}"));
        rows.add(row("x: huge exponent", "{\"status\":200,\"x\":1e999999}"));
        rows.add(row("x: -0.0e-0", "{\"status\":200,\"x\":-0.0e-0}"));

        StringBuilder sb = new StringBuilder();
        sb.append("# Generated by tests/java/.../GoldenGen.java from WasmAbi.decode. Do not edit.\n");
        sb.append("# name\\tinput(b64)\\tOK\\tstatus\\theaders\\tbody(b64)  |  name\\tinput(b64)\\tERR\\tdetail\n");
        sb.append("# headers: '-' for none, else ';'-joined key(b64)=v1(b64),v2(b64)\n");
        for (Row r : rows) {
            Object result = WasmAbi.decode(r.input());
            sb.append(r.name()).append('\t').append(B64.encodeToString(r.input())).append('\t');
            if (result instanceof io.flowcatalyst.sdk.result.Result.Ok<?, ?> ok) {
                WasmAbi.GuestReply reply = (WasmAbi.GuestReply) ok.value();
                sb.append("OK\t").append(reply.status()).append('\t').append(headers(reply.headers()))
                        .append('\t').append(B64.encodeToString(reply.body()));
            } else {
                var err = (io.flowcatalyst.sdk.result.Result.Err<?, ?>) result;
                sb.append("ERR\t").append(((WasmAbi.Malformed) err.error()).detail());
            }
            sb.append('\n');
        }
        Files.writeString(file, sb.toString());
    }

    private static String headers(Map<String, List<String>> headers) {
        if (headers.isEmpty()) return "-";
        List<String> parts = new ArrayList<>();
        headers.forEach((k, vs) -> {
            List<String> enc = new ArrayList<>();
            for (String v : vs) enc.add(B64.encodeToString(v.getBytes(StandardCharsets.UTF_8)));
            parts.add(B64.encodeToString(k.getBytes(StandardCharsets.UTF_8)) + "=" + String.join(",", enc));
        });
        return String.join(";", parts);
    }

    private static byte[] concat(String a, byte[] b, String c) {
        byte[] x = a.getBytes(StandardCharsets.UTF_8);
        byte[] z = c.getBytes(StandardCharsets.UTF_8);
        byte[] outb = new byte[x.length + b.length + z.length];
        System.arraycopy(x, 0, outb, 0, x.length);
        System.arraycopy(b, 0, outb, x.length, b.length);
        System.arraycopy(z, 0, outb, x.length + b.length, z.length);
        return outb;
    }

    // ── emit ────────────────────────────────────────────────────────────────

    private static void emitTable(Path file) throws Exception {
        Method emit = HostFunctions.class.getDeclaredMethod("emit", byte[].class, Events.class);
        emit.setAccessible(true);
        String ok = "{\"type\":\"app:sub:agg:evt\",\"dedupId\":\"d-1\"}";
        List<Object[]> rows = new ArrayList<>();
        rows.add(new Object[] {"minimal", ok, "capture"});
        rows.add(new Object[] {"full", "{\"type\":\"app:sub:agg:evt\",\"source\":\"src\",\"subject\":\"subj\","
                + "\"dataContentType\":\"application/json\",\"data\":{\"b\": 1, \"a\" : [true,null,\"x\"]},"
                + "\"correlationId\":\"corr\",\"causationId\":\"cause\",\"messageGroup\":\"grp\",\"dedupId\":\"d-2\","
                + "\"extra\":1}", "capture"});
        rows.add(new Object[] {"data null", "{\"type\":\"t\",\"dedupId\":\"d\",\"data\":null}", "capture"});
        rows.add(new Object[] {"data string", "{\"type\":\"t\",\"dedupId\":\"d\",\"data\":\"s\\u00e9/\\u0001\"}", "capture"});
        rows.add(new Object[] {"data raw non-ascii", "{\"type\":\"t\",\"dedupId\":\"d\",\"data\":[\"\u00e9\uD83D\uDE00\u2028\"]}", "capture"});
        rows.add(new Object[] {"data int", "{\"type\":\"t\",\"dedupId\":\"d\",\"data\":-12}", "capture"});
        rows.add(new Object[] {"data big int", "{\"type\":\"t\",\"dedupId\":\"d\",\"data\":123456789012345678901234567890}", "capture"});
        rows.add(new Object[] {"data float", "{\"type\":\"t\",\"dedupId\":\"d\",\"data\":1.50}", "capture"});
        rows.add(new Object[] {"data exponent", "{\"type\":\"t\",\"dedupId\":\"d\",\"data\":1e2}", "capture"});
        rows.add(new Object[] {"data empty object", "{\"type\":\"t\",\"dedupId\":\"d\",\"data\":{ }}", "capture"});
        rows.add(new Object[] {"data duplicate keys", "{\"type\":\"t\",\"dedupId\":\"d\",\"data\":{\"a\":1,\"b\":2,\"a\":3}}", "capture"});
        for (String n : List.of("-0", "-0.0", "0.0", "1e999", "-1e999", "5e-324", "4.9e-324", "1e-7", "0.001", "0.0001",
                "9999999.0", "10000000", "10000000.0", "123456789.125", "0.1", "1E+2", "2.5e-3", "1.7976931348623157e308",
                "100", "1e22", "1.0e23", "3.14159", "-2.5E-10", "12345678901234567890", "-9223372036854775808",
                "9223372036854775808", "2147483648", "1e0", "0.5e1", "123e-2", "1.25e-5", "0.30000000000000004",
                "2e-323", "1e-322", "9.5e-321")) {
            rows.add(new Object[] {"data number " + n, "{\"type\":\"t\",\"dedupId\":\"d\",\"data\":" + n + "}", "capture"});
        }
        rows.add(new Object[] {"data lone surrogate", "{\"type\":\"t\",\"dedupId\":\"d\",\"data\":\"\\ud800\"}", "capture"});
        rows.add(new Object[] {"data lone surrogate key", "{\"type\":\"t\",\"dedupId\":\"d\",\"data\":{\"\\udc00\":1}}", "capture"});
        rows.add(new Object[] {"type lone surrogate", "{\"type\":\"t\\ud800\",\"dedupId\":\"d\"}", "capture"});
        rows.add(new Object[] {"data control chars", "{\"type\":\"t\",\"dedupId\":\"d\",\"data\":\"\\u0000\\u001f\\u007f\\b\\f\\/\\u2028\"}", "capture"});
        rows.add(new Object[] {"data nested", "{\"type\":\"t\",\"dedupId\":\"d\",\"data\":{\"a\":{\"b\":[1,{\"c\":null}],\"d\":\"\"},\"e\":[]}}", "capture"});
        rows.add(new Object[] {"type blank unicode", "{\"type\":\"\\u2003\\u3000\\u001c\",\"dedupId\":\"d\"}", "capture"});
        rows.add(new Object[] {"type nbsp not blank", "{\"type\":\"\\u00a0\",\"dedupId\":\"d\"}", "capture"});
        rows.add(new Object[] {"type nel not blank", "{\"type\":\"\\u0085\",\"dedupId\":\"d\"}", "capture"});
        rows.add(new Object[] {"dedupId vertical tab blank", "{\"type\":\"t\",\"dedupId\":\"\\u000b\"}", "capture"});
        rows.add(new Object[] {"empty input", "", "capture"});
        rows.add(new Object[] {"null document", "null", "capture"});
        rows.add(new Object[] {"source not a string", "{\"type\":\"t\",\"dedupId\":\"d\",\"source\":5,\"subject\":null}", "capture"});
        rows.add(new Object[] {"type missing", "{\"dedupId\":\"d\"}", "capture"});
        rows.add(new Object[] {"type blank", "{\"type\":\" \",\"dedupId\":\"d\"}", "capture"});
        rows.add(new Object[] {"type number", "{\"type\":5,\"dedupId\":\"d\"}", "capture"});
        rows.add(new Object[] {"dedupId missing", "{\"type\":\"t\"}", "capture"});
        rows.add(new Object[] {"dedupId blank", "{\"type\":\"t\",\"dedupId\":\"\\t\"}", "capture"});
        rows.add(new Object[] {"dedupId number", "{\"type\":\"t\",\"dedupId\":1}", "capture"});
        rows.add(new Object[] {"both missing", "{}", "capture"});
        rows.add(new Object[] {"not an object", "[1]", "capture"});
        rows.add(new Object[] {"string document", "\"x\"", "capture"});
        rows.add(new Object[] {"not JSON", "nope", "capture"});
        rows.add(new Object[] {"trailing garbage", ok + " x", "capture"});
        rows.add(new Object[] {"platform refusal", ok, "refuse"});
        rows.add(new Object[] {"transport failure", ok, "unavailable"});
        rows.add(new Object[] {"unexpected exception", ok, "boom"});

        StringBuilder sb = new StringBuilder();
        sb.append("# Generated by tests/java/.../GoldenGen.java from HostFunctions.emit + the answer JSON. Do not edit.\n");
        sb.append("# name\\tmode\\tinput(b64)\\tanswer(b64)\\tevent: '-' or ','-joined fields (b64, '~' = null) in record order\n");
        for (Object[] r : rows) {
            String name = (String) r[0];
            byte[] input = ((String) r[1]).getBytes(StandardCharsets.UTF_8);
            String mode = (String) r[2];
            List<OutboundEvent> captured = new ArrayList<>();
            Events events = e -> {
                switch (mode) {
                    case "refuse" -> throw new EventEmitException("EVENT_TYPE_NOT_OWNED", 403);
                    case "unavailable" -> throw new EventEmitException("UNAVAILABLE", 503);
                    case "boom" -> throw new IllegalStateException("boom");
                    default -> captured.add(e);
                }
            };
            String error;
            try {
                error = (String) emit.invoke(null, input, events);
            } catch (java.lang.reflect.InvocationTargetException ite) {
                error = "THREW " + ite.getCause();
            }
            // The answer exactly as HostFunctions.emitEvent writes it.
            var answer = io.flowcatalyst.platform.shared.json.Json.MAPPER.createObjectNode();
            answer.put("ok", error == null);
            if (error != null) answer.put("error", error);
            byte[] answerBytes = io.flowcatalyst.platform.shared.json.Json.MAPPER.writeValueAsBytes(answer);
            sb.append(name).append('\t').append(mode).append('\t').append(B64.encodeToString(input)).append('\t')
                    .append(B64.encodeToString(answerBytes)).append('\t');
            if (captured.isEmpty()) {
                sb.append('-');
            } else {
                OutboundEvent e = captured.get(0);
                List<String> f = new ArrayList<>();
                for (String s : new String[] {e.type(), e.source(), e.subject(), e.dataContentType()}) f.add(enc(s));
                f.add(B64.encodeToString(e.data()));
                for (String s : new String[] {e.correlationId(), e.causationId(), e.messageGroup(), e.dedupId()}) f.add(enc(s));
                sb.append(String.join(",", f));
            }
            sb.append('\n');
        }
        Files.writeString(file, sb.toString());
    }

    private static String enc(String s) {
        return s == null ? "~" : B64.encodeToString(s.getBytes(StandardCharsets.UTF_8));
    }
}
