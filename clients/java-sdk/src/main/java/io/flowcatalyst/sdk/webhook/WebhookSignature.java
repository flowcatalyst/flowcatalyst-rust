package io.flowcatalyst.sdk.webhook;

import java.nio.charset.StandardCharsets;
import java.security.InvalidKeyException;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.time.Instant;
import java.time.format.DateTimeParseException;
import java.util.HexFormat;
import javax.crypto.Mac;
import javax.crypto.spec.SecretKeySpec;

/**
 * Verification for FlowCatalyst-signed deliveries: scheduled-job firings and
 * dispatch webhooks both carry
 *
 * <pre>
 *   X-FlowCatalyst-Signature: hex(HMAC-SHA256(timestamp + rawBody, secret))
 *   X-FlowCatalyst-Timestamp: 2026-08-07T09:32:12.123Z   (ms-precision ISO8601)
 * </pre>
 *
 * signed with your application service account's signing secret. Byte-format
 * matches the platform's router/dispatcher signers and the TypeScript/Laravel
 * SDK validators: 5-minute replay tolerance, 60-second future grace,
 * constant-time compare.
 *
 * <p>The verifier is framework-agnostic — hand it the raw body and the two
 * header values from whatever HTTP framework hosts your handler. The body
 * MUST be the raw bytes as received (sign-then-parse; a re-serialised JSON
 * body will not verify).
 */
public final class WebhookSignature {

    /** Says exactly what failed. */
    public enum ErrorCode {
        MISSING_SECRET,
        MISSING_SIGNATURE,
        MISSING_TIMESTAMP,
        INVALID_TIMESTAMP,
        TIMESTAMP_EXPIRED,
        TIMESTAMP_IN_FUTURE,
        INVALID_SIGNATURE,
        MISSING_BEARER,
        INVALID_BEARER
    }

    /** Verification failure; {@link #code()} says exactly what to fix. */
    public static final class WebhookSignatureException extends RuntimeException {
        private final ErrorCode code;

        public WebhookSignatureException(ErrorCode code, String message) {
            super(message);
            this.code = code;
        }

        public ErrorCode code() {
            return code;
        }
    }

    public static final int DEFAULT_TOLERANCE_SECONDS = 300;
    private static final int FUTURE_GRACE_SECONDS = 60;

    private WebhookSignature() {}

    /** Verify with the default 300s replay tolerance and no bearer gate. */
    public static void verify(byte[] rawBody, String signature, String timestamp, String secret) {
        verify(rawBody, signature, timestamp, secret, DEFAULT_TOLERANCE_SECONDS, null, null);
    }

    /**
     * Verify a signed delivery; throws {@link WebhookSignatureException} on
     * any failure (fail-closed — an empty secret is an error, not a bypass).
     *
     * @param rawBody             raw request body, exactly as received
     * @param signature           value of the X-FlowCatalyst-Signature header
     * @param timestamp           value of the X-FlowCatalyst-Timestamp header
     * @param secret              your application service account's signing secret
     * @param toleranceSeconds    max age before a delivery is considered a replay
     * @param expectedBearerToken optional second gate, AND-ed with the
     *                            signature (never a substitute): when set, the
     *                            Authorization header must be exactly
     *                            {@code Bearer <expectedBearerToken>}
     * @param authorization       value of the Authorization header (required
     *                            when {@code expectedBearerToken} is set)
     */
    public static void verify(
            byte[] rawBody,
            String signature,
            String timestamp,
            String secret,
            int toleranceSeconds,
            String expectedBearerToken,
            String authorization) {
        if (secret == null || secret.isEmpty()) {
            throw new WebhookSignatureException(
                    ErrorCode.MISSING_SECRET, "Webhook signing secret is not configured.");
        }
        if (signature == null || signature.isEmpty()) {
            throw new WebhookSignatureException(
                    ErrorCode.MISSING_SIGNATURE, "Missing X-FlowCatalyst-Signature header.");
        }
        if (timestamp == null || timestamp.isEmpty()) {
            throw new WebhookSignatureException(
                    ErrorCode.MISSING_TIMESTAMP, "Missing X-FlowCatalyst-Timestamp header.");
        }

        long webhookSeconds = parseTimestamp(timestamp);
        long nowSeconds = System.currentTimeMillis() / 1000;
        if (webhookSeconds < nowSeconds - toleranceSeconds) {
            throw new WebhookSignatureException(ErrorCode.TIMESTAMP_EXPIRED,
                    "Delivery timestamp older than " + toleranceSeconds + "s — replay rejected.");
        }
        if (webhookSeconds > nowSeconds + FUTURE_GRACE_SECONDS) {
            throw new WebhookSignatureException(ErrorCode.TIMESTAMP_IN_FUTURE,
                    "Delivery timestamp is in the future — check clock sync.");
        }

        // HMAC over the raw timestamp header string + raw body bytes.
        String expected = hmacHex(secret, timestamp, rawBody);
        byte[] a = expected.getBytes(StandardCharsets.UTF_8);
        byte[] b = signature.toLowerCase(java.util.Locale.ROOT).getBytes(StandardCharsets.UTF_8);
        if (a.length != b.length || !MessageDigest.isEqual(a, b)) {
            throw new WebhookSignatureException(ErrorCode.INVALID_SIGNATURE, "Signature mismatch.");
        }

        // Layered bearer gate (opt-in). Runs AFTER the signature so the strong
        // check is always exercised; both must pass when configured.
        if (expectedBearerToken != null && !expectedBearerToken.isEmpty()) {
            String prefix = "Bearer ";
            if (authorization == null || !authorization.startsWith(prefix)) {
                throw new WebhookSignatureException(
                        ErrorCode.MISSING_BEARER, "Missing Authorization bearer token.");
            }
            byte[] presented = authorization.substring(prefix.length())
                    .getBytes(StandardCharsets.UTF_8);
            byte[] want = expectedBearerToken.getBytes(StandardCharsets.UTF_8);
            if (presented.length != want.length || !MessageDigest.isEqual(presented, want)) {
                throw new WebhookSignatureException(
                        ErrorCode.INVALID_BEARER, "Invalid bearer token.");
            }
        }
    }

    /** Compute {@code hex(HMAC-SHA256(timestamp + body, secret))} — exposed for tests/signers. */
    public static String hmacHex(String secret, String timestamp, byte[] body) {
        try {
            Mac mac = Mac.getInstance("HmacSHA256");
            mac.init(new SecretKeySpec(secret.getBytes(StandardCharsets.UTF_8), "HmacSHA256"));
            mac.update(timestamp.getBytes(StandardCharsets.UTF_8));
            mac.update(body);
            return HexFormat.of().formatHex(mac.doFinal());
        } catch (NoSuchAlgorithmException | InvalidKeyException e) {
            throw new IllegalStateException("HmacSHA256 unavailable", e);
        }
    }

    /**
     * The platform emits millisecond-ISO8601 UTC; a bare Unix-seconds integer
     * is accepted for backward compatibility. The HMAC always covers the raw
     * header string — parsing only feeds the replay-window check.
     */
    private static long parseTimestamp(String timestamp) {
        if (timestamp.chars().allMatch(Character::isDigit)) {
            try {
                return Long.parseLong(timestamp);
            } catch (NumberFormatException e) {
                throw new WebhookSignatureException(ErrorCode.INVALID_TIMESTAMP,
                        "Unparseable X-FlowCatalyst-Timestamp '" + timestamp + "'.");
            }
        }
        try {
            return Instant.parse(timestamp).getEpochSecond();
        } catch (DateTimeParseException e) {
            throw new WebhookSignatureException(ErrorCode.INVALID_TIMESTAMP,
                    "Unparseable X-FlowCatalyst-Timestamp '" + timestamp + "'.");
        }
    }
}
