package io.flowcatalyst.sdk.webhook;

import static org.junit.jupiter.api.Assertions.assertDoesNotThrow;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;

import io.flowcatalyst.sdk.webhook.WebhookSignature.ErrorCode;
import io.flowcatalyst.sdk.webhook.WebhookSignature.WebhookSignatureException;
import java.nio.charset.StandardCharsets;
import java.time.Instant;
import org.junit.jupiter.api.Test;

class WebhookSignatureTest {

    private static final String SECRET = "test-signing-secret";
    private static final byte[] BODY = "{\"jobCode\":\"orders:ops:cleanup:run\"}"
            .getBytes(StandardCharsets.UTF_8);

    private static String now() {
        return Instant.now().toString();
    }

    @Test
    void acceptsValidSignature() {
        String ts = now();
        String sig = WebhookSignature.hmacHex(SECRET, ts, BODY);
        assertDoesNotThrow(() -> WebhookSignature.verify(BODY, sig, ts, SECRET));
    }

    @Test
    void signatureCompareIsCaseInsensitive() {
        String ts = now();
        String sig = WebhookSignature.hmacHex(SECRET, ts, BODY).toUpperCase();
        assertDoesNotThrow(() -> WebhookSignature.verify(BODY, sig, ts, SECRET));
    }

    @Test
    void knownVectorMatchesOtherSdks() {
        // Fixed vector shared with the TS/Laravel implementations:
        // HMAC-SHA256("2026-08-07T09:32:12.123Z" + "hello", "secret")
        assertEquals("a7d62aade7a79e88792f84e035b72788a3c78a30f46ad265ec8d6fb12a4e9f91",
                WebhookSignature.hmacHex("secret", "2026-08-07T09:32:12.123Z",
                        "hello".getBytes(StandardCharsets.UTF_8)));
    }

    @Test
    void failsClosedOnMissingInputs() {
        String ts = now();
        String sig = WebhookSignature.hmacHex(SECRET, ts, BODY);
        assertEquals(ErrorCode.MISSING_SECRET, code(() ->
                WebhookSignature.verify(BODY, sig, ts, "")));
        assertEquals(ErrorCode.MISSING_SIGNATURE, code(() ->
                WebhookSignature.verify(BODY, null, ts, SECRET)));
        assertEquals(ErrorCode.MISSING_TIMESTAMP, code(() ->
                WebhookSignature.verify(BODY, sig, "", SECRET)));
    }

    @Test
    void rejectsTamperedBodyAndBadTimestamps() {
        String ts = now();
        String sig = WebhookSignature.hmacHex(SECRET, ts, BODY);

        assertEquals(ErrorCode.INVALID_SIGNATURE, code(() ->
                WebhookSignature.verify("{}".getBytes(StandardCharsets.UTF_8), sig, ts, SECRET)));
        assertEquals(ErrorCode.INVALID_TIMESTAMP, code(() ->
                WebhookSignature.verify(BODY, sig, "not-a-time", SECRET)));

        String old = Instant.now().minusSeconds(600).toString();
        assertEquals(ErrorCode.TIMESTAMP_EXPIRED, code(() -> WebhookSignature.verify(
                BODY, WebhookSignature.hmacHex(SECRET, old, BODY), old, SECRET)));

        String future = Instant.now().plusSeconds(120).toString();
        assertEquals(ErrorCode.TIMESTAMP_IN_FUTURE, code(() -> WebhookSignature.verify(
                BODY, WebhookSignature.hmacHex(SECRET, future, BODY), future, SECRET)));
    }

    @Test
    void unixSecondsTimestampIsAcceptedForBackCompat() {
        String ts = String.valueOf(Instant.now().getEpochSecond());
        String sig = WebhookSignature.hmacHex(SECRET, ts, BODY);
        assertDoesNotThrow(() -> WebhookSignature.verify(BODY, sig, ts, SECRET));
    }

    @Test
    void bearerGateIsAndedWithTheSignature() {
        String ts = now();
        String sig = WebhookSignature.hmacHex(SECRET, ts, BODY);

        assertDoesNotThrow(() -> WebhookSignature.verify(
                BODY, sig, ts, SECRET, 300, "tok-123", "Bearer tok-123"));
        assertEquals(ErrorCode.MISSING_BEARER, code(() -> WebhookSignature.verify(
                BODY, sig, ts, SECRET, 300, "tok-123", null)));
        assertEquals(ErrorCode.INVALID_BEARER, code(() -> WebhookSignature.verify(
                BODY, sig, ts, SECRET, 300, "tok-123", "Bearer wrong")));
        // Wrong signature fails even with a correct bearer.
        assertEquals(ErrorCode.INVALID_SIGNATURE, code(() -> WebhookSignature.verify(
                BODY, "0".repeat(64), ts, SECRET, 300, "tok-123", "Bearer tok-123")));
    }

    private static ErrorCode code(Runnable runnable) {
        return assertThrows(WebhookSignatureException.class, runnable::run).code();
    }
}
