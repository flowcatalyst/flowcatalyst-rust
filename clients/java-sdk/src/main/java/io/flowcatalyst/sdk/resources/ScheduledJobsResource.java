package io.flowcatalyst.sdk.resources;

import io.flowcatalyst.sdk.generated.model.CompleteInstanceRequest;
import io.flowcatalyst.sdk.generated.model.CreateScheduledJobRequest;
import io.flowcatalyst.sdk.generated.model.CreatedResponse;
import io.flowcatalyst.sdk.generated.model.FireNowRequest;
import io.flowcatalyst.sdk.generated.model.FireNowResponse;
import io.flowcatalyst.sdk.generated.model.OffsetPageScheduledJobInstanceResponse;
import io.flowcatalyst.sdk.generated.model.OffsetPageScheduledJobResponse;
import io.flowcatalyst.sdk.generated.model.ScheduledJobInstanceLogResponse;
import io.flowcatalyst.sdk.generated.model.ScheduledJobInstanceResponse;
import io.flowcatalyst.sdk.generated.model.ScheduledJobResponse;
import io.flowcatalyst.sdk.generated.model.UpdateScheduledJobRequest;
import io.flowcatalyst.sdk.generated.model.WriteInstanceLogRequest;
import io.flowcatalyst.sdk.http.Transport;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Scheduled Jobs resource — CRUD + state transitions + history reads, plus
 * the SDK callbacks used by job handlers ({@link #logForInstance},
 * {@link #completeInstance}).
 */
public final class ScheduledJobsResource {

    private static final String PATH = "/api/scheduled-jobs";

    private final Transport transport;

    public ScheduledJobsResource(Transport transport) {
        this.transport = transport;
    }

    /** Optional filters for {@link #list}. */
    public record ListFilters(
            String clientId, String status, String search, Integer page, Integer size) {
        public static ListFilters none() {
            return new ListFilters(null, null, null, null, null);
        }
    }

    /** Optional filters for {@link #listInstances}. */
    public record InstanceFilters(
            String status, String triggerKind, String from, String to, Integer page, Integer size) {
        public static InstanceFilters none() {
            return new InstanceFilters(null, null, null, null, null, null);
        }
    }

    public CreatedResponse create(CreateScheduledJobRequest data) {
        return transport.post(PATH, data, CreatedResponse.class);
    }

    public OffsetPageScheduledJobResponse list(ListFilters filters) {
        Map<String, Object> query = new LinkedHashMap<>();
        if (filters != null) {
            query.put("clientId", filters.clientId());
            query.put("status", filters.status());
            query.put("search", filters.search());
            query.put("page", filters.page());
            query.put("size", filters.size());
        }
        return transport.get(PATH, query, OffsetPageScheduledJobResponse.class);
    }

    public ScheduledJobResponse get(String id) {
        return transport.get(PATH + "/" + Transport.enc(id), null, ScheduledJobResponse.class);
    }

    public ScheduledJobResponse getByCode(String code) {
        return transport.get(
                PATH + "/by-code/" + Transport.enc(code), null, ScheduledJobResponse.class);
    }

    public void update(String id, UpdateScheduledJobRequest data) {
        transport.put(PATH + "/" + Transport.enc(id), data, Void.class);
    }

    public void pause(String id) {
        transport.post(PATH + "/" + Transport.enc(id) + "/pause", null, Void.class);
    }

    public void resume(String id) {
        transport.post(PATH + "/" + Transport.enc(id) + "/resume", null, Void.class);
    }

    public void archive(String id) {
        transport.post(PATH + "/" + Transport.enc(id) + "/archive", null, Void.class);
    }

    public void delete(String id) {
        transport.delete(PATH + "/" + Transport.enc(id), Void.class);
    }

    /** Fire a job immediately, outside its cron schedule. */
    public FireNowResponse fire(String id, FireNowRequest request) {
        return transport.post(
                PATH + "/" + Transport.enc(id) + "/fire",
                request != null ? request : new FireNowRequest(),
                FireNowResponse.class);
    }

    public OffsetPageScheduledJobInstanceResponse listInstances(String id, InstanceFilters filters) {
        Map<String, Object> query = new LinkedHashMap<>();
        if (filters != null) {
            query.put("status", filters.status());
            query.put("triggerKind", filters.triggerKind());
            query.put("from", filters.from());
            query.put("to", filters.to());
            query.put("page", filters.page());
            query.put("size", filters.size());
        }
        return transport.get(
                PATH + "/" + Transport.enc(id) + "/instances",
                query,
                OffsetPageScheduledJobInstanceResponse.class);
    }

    public ScheduledJobInstanceResponse getInstance(String instanceId) {
        return transport.get(
                PATH + "/instances/" + Transport.enc(instanceId),
                null,
                ScheduledJobInstanceResponse.class);
    }

    public List<ScheduledJobInstanceLogResponse> listInstanceLogs(String instanceId) {
        return transport.get(
                PATH + "/instances/" + Transport.enc(instanceId) + "/logs",
                null,
                transport.listOf(ScheduledJobInstanceLogResponse.class));
    }

    /** Append a log line to a running instance (job-handler callback). */
    public void logForInstance(String instanceId, WriteInstanceLogRequest request) {
        transport.post(
                PATH + "/instances/" + Transport.enc(instanceId) + "/log", request, Void.class);
    }

    /** Report completion of a tracked instance (job-handler callback). */
    public void completeInstance(String instanceId, CompleteInstanceRequest request) {
        transport.post(
                PATH + "/instances/" + Transport.enc(instanceId) + "/complete", request, Void.class);
    }
}
