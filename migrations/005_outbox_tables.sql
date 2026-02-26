-- FlowCatalyst Outbox/Projection Feed Tables
-- Matches TypeScript Drizzle schema exactly

-- =============================================================================
-- msg_event_projection_feed - Event projection feed (CQRS)
-- =============================================================================
CREATE TABLE IF NOT EXISTS msg_event_projection_feed (
    id BIGSERIAL PRIMARY KEY,
    event_id VARCHAR(13) NOT NULL,
    payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    processed SMALLINT NOT NULL DEFAULT 0,
    processed_at TIMESTAMPTZ,
    error_message TEXT
);

CREATE INDEX IF NOT EXISTS idx_msg_event_projection_feed_unprocessed ON msg_event_projection_feed (id) WHERE processed = 0;
CREATE INDEX IF NOT EXISTS idx_msg_event_projection_feed_in_progress ON msg_event_projection_feed (id) WHERE processed = 9;

-- =============================================================================
-- msg_dispatch_job_projection_feed - Dispatch job projection feed (CQRS)
-- =============================================================================
CREATE TABLE IF NOT EXISTS msg_dispatch_job_projection_feed (
    id BIGSERIAL PRIMARY KEY,
    dispatch_job_id VARCHAR(13) NOT NULL,
    operation VARCHAR(10) NOT NULL,
    payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    processed SMALLINT NOT NULL DEFAULT 0,
    processed_at TIMESTAMPTZ,
    error_message TEXT
);

CREATE INDEX IF NOT EXISTS idx_msg_dj_projection_feed_unprocessed ON msg_dispatch_job_projection_feed (dispatch_job_id, id) WHERE processed = 0;
CREATE INDEX IF NOT EXISTS idx_msg_dj_projection_feed_in_progress ON msg_dispatch_job_projection_feed (id) WHERE processed = 9;
CREATE INDEX IF NOT EXISTS idx_msg_dj_projection_feed_processed_at ON msg_dispatch_job_projection_feed (processed_at) WHERE processed = 1;
