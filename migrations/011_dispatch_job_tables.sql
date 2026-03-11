-- FlowCatalyst Dispatch Job Tables
-- NOTE: These tables are superseded by 004_messaging_tables.sql which creates
-- msg_dispatch_jobs, msg_dispatch_jobs_read, and msg_dispatch_job_attempts
-- (matching the TypeScript reference exactly).
-- The tables below were from an earlier Rust-only migration with a different schema
-- (JSONB attempts, VARCHAR(17) IDs) and are NOT used because CREATE TABLE IF NOT EXISTS
-- is silently skipped when the tables already exist from 004.
-- The Rust code now targets the 004 schema (VARCHAR(13) IDs, normalized attempts table).

-- SUPERSEDED: Already created by 004_messaging_tables.sql
CREATE TABLE IF NOT EXISTS msg_dispatch_jobs (
    id VARCHAR(17) PRIMARY KEY,
    external_id VARCHAR(255),
    kind VARCHAR(20) NOT NULL DEFAULT 'EVENT',
    code VARCHAR(255) NOT NULL,
    source VARCHAR(255) NOT NULL,
    subject VARCHAR(255),
    target_url TEXT NOT NULL,
    protocol VARCHAR(20) NOT NULL DEFAULT 'HTTP_WEBHOOK',
    payload TEXT NOT NULL,
    payload_content_type VARCHAR(100) NOT NULL DEFAULT 'application/json',
    data_only BOOLEAN NOT NULL DEFAULT FALSE,
    event_id VARCHAR(17),
    correlation_id VARCHAR(255),
    client_id VARCHAR(17),
    subscription_id VARCHAR(17),
    service_account_id VARCHAR(17),
    dispatch_pool_id VARCHAR(17),
    message_group VARCHAR(255),
    mode VARCHAR(20) NOT NULL DEFAULT 'IMMEDIATE',
    sequence INTEGER NOT NULL DEFAULT 99,
    timeout_seconds INTEGER NOT NULL DEFAULT 30,
    max_retries INTEGER NOT NULL DEFAULT 3,
    retry_strategy VARCHAR(30) NOT NULL DEFAULT 'EXPONENTIAL_BACKOFF',
    status VARCHAR(20) NOT NULL DEFAULT 'PENDING',
    attempt_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    attempts JSONB NOT NULL DEFAULT '[]',
    metadata JSONB NOT NULL DEFAULT '[]',
    idempotency_key VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    queued_at TIMESTAMPTZ,
    last_attempt_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    duration_millis BIGINT,
    next_retry_at TIMESTAMPTZ
);

-- SUPERSEDED: Already created by 004_messaging_tables.sql
CREATE TABLE IF NOT EXISTS msg_dispatch_jobs_read (
    id VARCHAR(17) PRIMARY KEY,
    external_id VARCHAR(255),
    source VARCHAR(255) NOT NULL,
    kind VARCHAR(20) NOT NULL DEFAULT 'EVENT',
    code VARCHAR(255) NOT NULL,
    subject VARCHAR(255),
    event_id VARCHAR(17),
    correlation_id VARCHAR(255),
    target_url TEXT NOT NULL,
    protocol VARCHAR(20) NOT NULL DEFAULT 'HTTP_WEBHOOK',
    client_id VARCHAR(17),
    subscription_id VARCHAR(17),
    service_account_id VARCHAR(17),
    dispatch_pool_id VARCHAR(17),
    message_group VARCHAR(255),
    mode VARCHAR(20) NOT NULL DEFAULT 'IMMEDIATE',
    sequence INTEGER NOT NULL DEFAULT 99,
    status VARCHAR(20) NOT NULL DEFAULT 'PENDING',
    attempt_count INTEGER NOT NULL DEFAULT 0,
    max_retries INTEGER NOT NULL DEFAULT 3,
    last_error TEXT,
    timeout_seconds INTEGER NOT NULL DEFAULT 30,
    retry_strategy VARCHAR(30) NOT NULL DEFAULT 'EXPONENTIAL_BACKOFF',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    scheduled_for TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    last_attempt_at TIMESTAMPTZ,
    duration_millis BIGINT,
    idempotency_key VARCHAR(255),
    is_completed BOOLEAN NOT NULL DEFAULT FALSE,
    is_terminal BOOLEAN NOT NULL DEFAULT FALSE,
    projected_at TIMESTAMPTZ
);
