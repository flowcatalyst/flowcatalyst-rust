package io.flowcatalyst.sdk.sync;

import java.util.List;

/**
 * Aggregate result of syncing a full {@link Definitions.DefinitionSet}. Each
 * category is either {@link Category.Synced} (mirroring the platform's
 * {@code SyncResultResponse}), the {@link Category.Skipped} sentinel when the
 * category wasn't part of the submitted set, or {@link Category.Failed} when
 * a LOCAL check (e.g. a duplicate code within one sync scope) or a caught
 * HTTP failure stopped it — currently connections, subscriptions and
 * scheduled jobs can report {@code Failed} this way; every other category
 * still lets its HTTP exception propagate, unchanged from before.
 */
public record SyncResult(
        String applicationCode,
        Category roles,
        Category eventTypes,
        Category subscriptions,
        Category dispatchPools,
        Category principals,
        Category processes,
        Category scheduledJobs,
        /*
         * OpenAPI sync is a single-document upload: on success syncedCodes
         * carries [version]; created/updated reflect newly-published vs
         * replaced (both zero on a byte-identical re-sync).
         */
        Category openapi,
        /*
         * Connections are synced BEFORE subscriptions (a subscription's
         * connectionCode must resolve in the same run) — appended as the
         * LAST component, not inserted before subscriptions, so the
         * pre-existing 9-component constructor below keeps compiling.
         */
        Category connections) {

    /**
     * The pre-connections component list, kept so existing callers of the
     * canonical constructor keep compiling.
     */
    public SyncResult(
            String applicationCode,
            Category roles,
            Category eventTypes,
            Category subscriptions,
            Category dispatchPools,
            Category principals,
            Category processes,
            Category scheduledJobs,
            Category openapi) {
        this(applicationCode, roles, eventTypes, subscriptions, dispatchPools, principals, processes,
                scheduledJobs, openapi, Category.SKIPPED);
    }

    /** Per-category outcome. */
    public sealed interface Category {

        record Synced(
                String applicationCode,
                int created,
                int updated,
                int deleted,
                List<String> syncedCodes)
                implements Category {}

        record Skipped() implements Category {}

        /**
         * A category that could not be synced: either a local validation
         * failure (e.g. two definitions in the same scope sharing a code)
         * that meant nothing was sent, or an HTTP failure caught so sibling
         * scopes could still be attempted (e.g. a connection sync failing
         * skips just that scope's subscriptions). {@code created}/{@code
         * updated}/{@code deleted}/{@code syncedCodes} reflect any OTHER
         * scope that synced successfully before or alongside the failure —
         * they are not necessarily all zero.
         */
        record Failed(
                int created,
                int updated,
                int deleted,
                List<String> syncedCodes,
                String error)
                implements Category {}

        Category SKIPPED = new Skipped();

        default boolean isSynced() {
            return this instanceof Synced;
        }

        default boolean isFailed() {
            return this instanceof Failed;
        }
    }
}
