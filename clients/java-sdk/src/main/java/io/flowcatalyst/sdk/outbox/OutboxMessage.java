package io.flowcatalyst.sdk.outbox;

import java.time.Instant;
import java.util.Map;

/**
 * An outbox message record to be persisted by the driver. The schema must
 * match the outbox poller's expected format — the poller reads these rows and
 * manages status transitions.
 */
public record OutboxMessage(
        String id,
        MessageType type,
        String messageGroup,
        String payload,
        int status,
        Instant createdAt,
        Instant updatedAt,
        /* SDK-specific: client identifier for multi-tenant routing. */
        String clientId,
        /* SDK-specific: byte size of payload. */
        int payloadSize,
        /* SDK-specific: optional headers (null when none). */
        Map<String, String> headers) {

    /** Message types supported by the outbox. */
    public enum MessageType {
        EVENT,
        DISPATCH_JOB,
        AUDIT_LOG
    }

    /**
     * Outbox status codes matching the outbox poller. The wire contract uses
     * SMALLINT status codes, NOT strings. Only {@link #PENDING} is written by
     * the SDK; all others are set by the poller.
     */
    public static final class Status {
        /** Waiting to be processed. */
        public static final int PENDING = 0;
        /** Successfully sent to FlowCatalyst. */
        public static final int SUCCESS = 1;
        /** API returned 400 Bad Request (permanent failure). */
        public static final int BAD_REQUEST = 2;
        /** API returned 500 Internal Server Error (retryable). */
        public static final int INTERNAL_ERROR = 3;
        /** API returned 401 Unauthorized (retryable). */
        public static final int UNAUTHORIZED = 4;
        /** API returned 403 Forbidden (permanent failure). */
        public static final int FORBIDDEN = 5;
        /** API returned 502/503/504 Gateway Error (retryable). */
        public static final int GATEWAY_ERROR = 6;
        /** Currently being processed — crash recovery marker. */
        public static final int IN_PROGRESS = 9;

        private Status() {}
    }
}
