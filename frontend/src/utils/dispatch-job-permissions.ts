/**
 * Permissions the dispatch job pages check before offering an action the
 * API would refuse (internal/platform/dispatchjob/api: viewRawPerm).
 */

/** Sign (POST /api/dispatch-jobs/{id}/sign) and the raw views need this. */
export const DISPATCH_JOB_VIEW_RAW = "platform:messaging:dispatch-job:view-raw";
