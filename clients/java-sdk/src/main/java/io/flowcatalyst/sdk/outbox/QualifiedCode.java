package io.flowcatalyst.sdk.outbox;

/**
 * Every code the platform facets on — event {@code type}s and dispatch-job
 * {@code code}s — must be a fully qualified
 * {@code application:subdomain:aggregate:action} string. The platform projects
 * the first three segments out as application/subdomain/aggregate facets; a
 * bare one-word code both renders as {@code name:::} in the UI and facets
 * under the WRONG application (segment 1), and it denies the delivery pipeline
 * the application linkage it uses to resolve signing credentials. The SDK
 * therefore refuses to emit unqualified codes.
 */
public final class QualifiedCode {

    private QualifiedCode() {}

    public static void assertQualified(String value, String field) {
        String[] segments = value == null ? new String[0] : value.split(":", -1);
        boolean valid = segments.length == 4;
        if (valid) {
            for (String segment : segments) {
                if (segment.trim().isEmpty()) {
                    valid = false;
                    break;
                }
            }
        }
        if (!valid) {
            throw new IllegalArgumentException(
                    field + " '" + value + "' must be a fully qualified code with four non-empty "
                            + "colon-separated segments: application:subdomain:aggregate:action "
                            + "(e.g. 'fulfil-go:fulfilment:part:create-pick')");
        }
    }
}
