package io.flowcatalyst.sdk.sync;

import java.util.Set;

/**
 * Options for a sync call.
 *
 * @param removeUnlisted when true, the platform removes SDK-sourced rows not
 *                       present in the submitted list (per category); rows
 *                       created through the admin UI are preserved regardless
 * @param skip           per-category opt-out — omitting a category from the
 *                       set already skips it; these force-skip categories even
 *                       when present (e.g. to stage a rollout)
 */
public record SyncOptions(boolean removeUnlisted, Set<SyncCategory> skip) {

    public enum SyncCategory {
        ROLES,
        EVENT_TYPES,
        CONNECTIONS,
        SUBSCRIPTIONS,
        DISPATCH_POOLS,
        PRINCIPALS,
        PROCESSES,
        SCHEDULED_JOBS,
        OPENAPI
    }

    public static SyncOptions defaults() {
        return new SyncOptions(false, Set.of());
    }

    public static SyncOptions removingUnlisted() {
        return new SyncOptions(true, Set.of());
    }

    public SyncOptions skipping(SyncCategory... categories) {
        return new SyncOptions(removeUnlisted, Set.of(categories));
    }

    boolean skips(SyncCategory category) {
        return skip != null && skip.contains(category);
    }
}
