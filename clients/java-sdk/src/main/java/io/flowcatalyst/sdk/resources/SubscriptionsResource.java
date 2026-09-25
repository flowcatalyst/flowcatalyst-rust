package io.flowcatalyst.sdk.resources;

import io.flowcatalyst.sdk.generated.model.CreateSubscriptionRequest;
import io.flowcatalyst.sdk.generated.model.CreatedResponse;
import io.flowcatalyst.sdk.generated.model.SubscriptionListResponse;
import io.flowcatalyst.sdk.generated.model.SubscriptionResponse;
import io.flowcatalyst.sdk.generated.model.SyncResultResponse;
import io.flowcatalyst.sdk.generated.model.SyncSubscriptionInputRequest;
import io.flowcatalyst.sdk.generated.model.SyncSubscriptionsRequest;
import io.flowcatalyst.sdk.generated.model.UpdateSubscriptionRequest;
import io.flowcatalyst.sdk.http.Transport;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/** Subscriptions resource — manage event subscriptions. */
public final class SubscriptionsResource {

    private final Transport transport;

    public SubscriptionsResource(Transport transport) {
        this.transport = transport;
    }

    /** List subscriptions, optionally filtered by status and/or client. */
    public SubscriptionListResponse list(String status, String clientId) {
        Map<String, Object> query = new LinkedHashMap<>();
        query.put("status", status);
        query.put("clientId", clientId);
        return transport.get("/api/subscriptions", query, SubscriptionListResponse.class);
    }

    public SubscriptionResponse get(String id) {
        return transport.get(
                "/api/subscriptions/" + Transport.enc(id), null, SubscriptionResponse.class);
    }

    public CreatedResponse create(CreateSubscriptionRequest data) {
        return transport.post("/api/subscriptions", data, CreatedResponse.class);
    }

    public void update(String id, UpdateSubscriptionRequest data) {
        transport.put("/api/subscriptions/" + Transport.enc(id), data, Void.class);
    }

    public void delete(String id) {
        transport.delete("/api/subscriptions/" + Transport.enc(id), Void.class);
    }

    public void pause(String id) {
        transport.post("/api/subscriptions/" + Transport.enc(id) + "/pause", null, Void.class);
    }

    public void resume(String id) {
        transport.post("/api/subscriptions/" + Transport.enc(id) + "/resume", null, Void.class);
    }

    /**
     * Sync subscriptions for an application.
     * Calls {@code POST /api/applications/{appCode}/subscriptions/sync}.
     */
    public SyncResultResponse sync(
            String applicationCode,
            List<SyncSubscriptionInputRequest> subscriptions,
            boolean removeUnlisted) {
        SyncSubscriptionsRequest body = new SyncSubscriptionsRequest().subscriptions(subscriptions);
        return transport.post(
                "/api/applications/" + Transport.enc(applicationCode) + "/subscriptions/sync",
                Map.of("removeUnlisted", removeUnlisted),
                body,
                SyncResultResponse.class);
    }
}
