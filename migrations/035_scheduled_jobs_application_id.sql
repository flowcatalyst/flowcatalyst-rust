-- msg_scheduled_jobs.application_id, as Java has it (V1__baseline.sql,
-- ../flowcatalyst-javalin at 0118cdca): the application a job belongs to.
-- Nullable: a job made through the scheduled-job API or an SDK sync names no
-- application. A function's schedule (promote wiring) names the function's,
-- and its firings are signed with that application's credentials, as Java's
-- JobDispatcher signs them.
ALTER TABLE msg_scheduled_jobs ADD COLUMN IF NOT EXISTS application_id VARCHAR(17);

CREATE INDEX IF NOT EXISTS idx_msg_scheduled_jobs_application_id
    ON msg_scheduled_jobs (application_id);
