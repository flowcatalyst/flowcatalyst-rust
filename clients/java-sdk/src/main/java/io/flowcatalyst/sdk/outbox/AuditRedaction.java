package io.flowcatalyst.sdk.outbox;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.node.ArrayNode;
import com.fasterxml.jackson.databind.node.JsonNodeFactory;
import com.fasterxml.jackson.databind.node.ObjectNode;
import java.util.Iterator;
import java.util.Locale;
import java.util.Map;
import java.util.Set;

/**
 * The audit-redaction rule: an audit row never carries a password, secret
 * or token in the clear, wherever it sits in the command document.
 *
 * <p>The TypeScript, Laravel and Rust SDKs apply the same rule, and every
 * implementation runs the shared {@code audit-redaction-vectors.json} cases.
 * A key is secret when, lower-cased with {@code _} and {@code -} removed, it
 * ends with {@code password}, {@code passwordhash}, {@code secret},
 * {@code secretref}, {@code passphrase} or {@code token}, or equals
 * {@code apikey}, {@code privatekey}, {@code authorization} or
 * {@code cookie}. A secret key's value becomes {@code "***"} whatever its
 * type; {@code null} and booleans are kept. Masked fields are top-level
 * names masked the same way even when the name rule would keep them.
 */
public final class AuditRedaction {

    private static final JsonNodeFactory NODES = JsonNodeFactory.instance;
    private static final String MASK = "***";

    private static final Set<String> SECRET_SUFFIXES = Set.of(
            "password", "passwordhash", "secret", "secretref", "passphrase", "token");

    private static final Set<String> SECRET_EXACT = Set.of(
            "apikey", "privatekey", "authorization", "cookie");

    private AuditRedaction() {}

    /**
     * Redact {@code document}, walking objects and arrays. The input is never
     * mutated: a fresh tree is returned, sharing only unmasked leaf values.
     *
     * @param document the command/operation document ({@code null} passes through)
     * @param masked   top-level field names to mask on top of the name rule
     */
    public static JsonNode redact(JsonNode document, Set<String> masked) {
        if (document == null) return null;
        return redactNode(document, masked == null ? Set.of() : masked, true);
    }

    /** Whether {@code key} is secret under the name rule. */
    public static boolean isSecretKey(String key) {
        String normalized = key.toLowerCase(Locale.ROOT).replace("_", "").replace("-", "");
        if (SECRET_EXACT.contains(normalized)) return true;
        for (String suffix : SECRET_SUFFIXES) {
            if (normalized.endsWith(suffix)) return true;
        }
        return false;
    }

    private static JsonNode redactNode(JsonNode node, Set<String> maskedTopLevel, boolean topLevel) {
        if (node.isObject()) {
            ObjectNode out = NODES.objectNode();
            Iterator<Map.Entry<String, JsonNode>> fields = node.fields();
            while (fields.hasNext()) {
                Map.Entry<String, JsonNode> entry = fields.next();
                String key = entry.getKey();
                JsonNode value = entry.getValue();
                boolean mask = isSecretKey(key) || (topLevel && maskedTopLevel.contains(key));
                out.set(key, mask ? maskValue(value) : redactNode(value, maskedTopLevel, false));
            }
            return out;
        }
        if (node.isArray()) {
            ArrayNode out = NODES.arrayNode(node.size());
            for (JsonNode element : node) {
                out.add(redactNode(element, maskedTopLevel, false));
            }
            return out;
        }
        return node;
    }

    private static JsonNode maskValue(JsonNode value) {
        if (value == null || value.isNull() || value.isBoolean()) return value;
        return NODES.textNode(MASK);
    }
}
