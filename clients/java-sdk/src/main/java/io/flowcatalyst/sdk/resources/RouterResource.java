package io.flowcatalyst.sdk.resources;

import com.fasterxml.jackson.annotation.JsonIgnoreProperties;
import io.flowcatalyst.sdk.http.Transport;
import java.util.List;
import java.util.Map;

/**
 * Router monitoring resource — talks to the message router (a separate
 * process from the platform) at the configured router base URL, without
 * authentication, mirroring the TypeScript SDK.
 *
 * <p>Designed for an external recovery / replay process that maintains its
 * own list of "messages that look stuck" and wants to confirm whether the
 * router is still actively processing each one before re-enqueueing.
 */
public final class RouterResource {

    /**
     * Hard cap on batch size, mirrors the server-side limit. Larger arrays
     * will be rejected with HTTP 400 by the router.
     */
    public static final int IN_PIPELINE_CHECK_BATCH_LIMIT = 5000;

    private final Transport transport;
    private final String routerBaseUrl;

    public RouterResource(Transport transport, String routerBaseUrl) {
        this.transport = transport;
        this.routerBaseUrl = routerBaseUrl;
    }

    @JsonIgnoreProperties(ignoreUnknown = true)
    public record InPipelineDetail(
            String messageId,
            String brokerMessageId,
            String queueId,
            String poolCode,
            long elapsedTimeMs,
            String addedToInPipelineAt) {}

    @JsonIgnoreProperties(ignoreUnknown = true)
    public record InPipelineCheckResponse(
            String messageId, boolean inPipeline, InPipelineDetail detail) {}

    /**
     * Check whether a single application message ID is currently held in the
     * router's in-pipeline map. O(1) on the server side. Always returns 200 —
     * {@code inPipeline=false} is a normal answer.
     */
    public InPipelineCheckResponse inPipeline(String messageId) {
        return transport.rawUnauthenticated(
                "GET",
                routerBaseUrl + "/monitoring/in-flight-messages/check",
                Map.of("messageId", messageId),
                null,
                transport.mapper().getTypeFactory().constructType(InPipelineCheckResponse.class));
    }

    /**
     * Batch-check whether each given application message ID is currently held
     * in the router's in-pipeline map. Returns {@code messageId → bool} (true
     * = router has it, do not resend). The server caps the batch at
     * {@link #IN_PIPELINE_CHECK_BATCH_LIMIT} ids; split larger batches
     * client-side before calling.
     */
    public Map<String, Boolean> inPipelineBatch(List<String> messageIds) {
        return transport.rawUnauthenticated(
                "POST",
                routerBaseUrl + "/monitoring/in-flight-messages/check-batch",
                null,
                Map.of("messageIds", messageIds),
                transport.mapOf(String.class, Boolean.class));
    }
}
