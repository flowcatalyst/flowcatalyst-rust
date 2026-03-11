-- FlowCatalyst Messaging Tables
-- Matches TypeScript Drizzle schema exactly

-- =============================================================================
-- msg_events - Events (write model)
-- =============================================================================
CREATE TABLE IF NOT EXISTS msg_events (
    id VARCHAR(17) PRIMARY KEY,
    spec_version VARCHAR(20) DEFAULT '1.0',
    type VARCHAR(200) NOT NULL,
    source VARCHAR(500) NOT NULL,
    subject VARCHAR(500),
    time TIMESTAMPTZ NOT NULL,
    data JSONB,
    correlation_id VARCHAR(100),
    causation_id VARCHAR(100),
    deduplication_id VARCHAR(200),
    message_group VARCHAR(200),
    client_id VARCHAR(17),
    context_data JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_msg_events_type ON msg_events (type);
CREATE INDEX IF NOT EXISTS idx_msg_events_client_type ON msg_events (client_id, type);
CREATE INDEX IF NOT EXISTS idx_msg_events_time ON msg_events (time);
CREATE INDEX IF NOT EXISTS idx_msg_events_correlation ON msg_events (correlation_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_msg_events_deduplication ON msg_events (deduplication_id);

-- =============================================================================
-- msg_events_read - Events read projection (CQRS)
-- =============================================================================
CREATE TABLE IF NOT EXISTS msg_events_read (
    id VARCHAR(17) PRIMARY KEY,
    spec_version VARCHAR(20),
    type VARCHAR(200) NOT NULL,
    source VARCHAR(500) NOT NULL,
    subject VARCHAR(500),
    time TIMESTAMPTZ NOT NULL,
    data TEXT,
    correlation_id VARCHAR(100),
    causation_id VARCHAR(100),
    deduplication_id VARCHAR(200),
    message_group VARCHAR(200),
    client_id VARCHAR(17),
    application VARCHAR(100),
    subdomain VARCHAR(100),
    aggregate VARCHAR(100),
    projected_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_msg_events_read_type ON msg_events_read (type);
CREATE INDEX IF NOT EXISTS idx_msg_events_read_client_id ON msg_events_read (client_id);
CREATE INDEX IF NOT EXISTS idx_msg_events_read_time ON msg_events_read (time);
CREATE INDEX IF NOT EXISTS idx_msg_events_read_application ON msg_events_read (application);
CREATE INDEX IF NOT EXISTS idx_msg_events_read_subdomain ON msg_events_read (subdomain);
CREATE INDEX IF NOT EXISTS idx_msg_events_read_aggregate ON msg_events_read (aggregate);
CREATE INDEX IF NOT EXISTS idx_msg_events_read_correlation_id ON msg_events_read (correlation_id);

-- =============================================================================
-- msg_dispatch_jobs - Dispatch jobs (write model)
-- =============================================================================
CREATE TABLE IF NOT EXISTS msg_dispatch_jobs (
    id VARCHAR(17) PRIMARY KEY,
    external_id VARCHAR(100),
    source VARCHAR(500),
    kind VARCHAR(20) NOT NULL DEFAULT 'EVENT',
    code VARCHAR(200) NOT NULL,
    subject VARCHAR(500),
    event_id VARCHAR(17),
    correlation_id VARCHAR(100),
    metadata JSONB DEFAULT '[]'::jsonb,
    target_url VARCHAR(500) NOT NULL,
    protocol VARCHAR(30) NOT NULL DEFAULT 'HTTP_WEBHOOK',
    payload TEXT,
    payload_content_type VARCHAR(100) DEFAULT 'application/json',
    data_only BOOLEAN NOT NULL DEFAULT TRUE,
    service_account_id VARCHAR(17),
    client_id VARCHAR(17),
    subscription_id VARCHAR(17),
    mode VARCHAR(30) NOT NULL DEFAULT 'IMMEDIATE',
    dispatch_pool_id VARCHAR(17),
    message_group VARCHAR(200),
    sequence INTEGER NOT NULL DEFAULT 99,
    timeout_seconds INTEGER NOT NULL DEFAULT 30,
    schema_id VARCHAR(17),
    status VARCHAR(20) NOT NULL DEFAULT 'PENDING',
    max_retries INTEGER NOT NULL DEFAULT 3,
    retry_strategy VARCHAR(50) DEFAULT 'exponential',
    scheduled_for TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    last_attempt_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    duration_millis BIGINT,
    last_error TEXT,
    idempotency_key VARCHAR(100),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_msg_dispatch_jobs_status ON msg_dispatch_jobs (status);
CREATE INDEX IF NOT EXISTS idx_msg_dispatch_jobs_client_id ON msg_dispatch_jobs (client_id);
CREATE INDEX IF NOT EXISTS idx_msg_dispatch_jobs_message_group ON msg_dispatch_jobs (message_group);
CREATE INDEX IF NOT EXISTS idx_msg_dispatch_jobs_subscription_id ON msg_dispatch_jobs (subscription_id);
CREATE INDEX IF NOT EXISTS idx_msg_dispatch_jobs_created_at ON msg_dispatch_jobs (created_at);
CREATE INDEX IF NOT EXISTS idx_msg_dispatch_jobs_scheduled_for ON msg_dispatch_jobs (scheduled_for);

-- =============================================================================
-- msg_dispatch_jobs_read - Dispatch jobs read projection (CQRS)
-- =============================================================================
CREATE TABLE IF NOT EXISTS msg_dispatch_jobs_read (
    id VARCHAR(17) PRIMARY KEY,
    external_id VARCHAR(100),
    source VARCHAR(500),
    kind VARCHAR(20) NOT NULL,
    code VARCHAR(200) NOT NULL,
    subject VARCHAR(500),
    event_id VARCHAR(17),
    correlation_id VARCHAR(100),
    target_url VARCHAR(500) NOT NULL,
    protocol VARCHAR(30) NOT NULL,
    service_account_id VARCHAR(17),
    client_id VARCHAR(17),
    subscription_id VARCHAR(17),
    dispatch_pool_id VARCHAR(17),
    mode VARCHAR(30) NOT NULL,
    message_group VARCHAR(200),
    sequence INTEGER DEFAULT 99,
    timeout_seconds INTEGER DEFAULT 30,
    status VARCHAR(20) NOT NULL,
    max_retries INTEGER NOT NULL,
    retry_strategy VARCHAR(50),
    scheduled_for TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    last_attempt_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    duration_millis BIGINT,
    last_error TEXT,
    idempotency_key VARCHAR(100),
    is_completed BOOLEAN,
    is_terminal BOOLEAN,
    application VARCHAR(100),
    subdomain VARCHAR(100),
    aggregate VARCHAR(100),
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    projected_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_msg_dispatch_jobs_read_status ON msg_dispatch_jobs_read (status);
CREATE INDEX IF NOT EXISTS idx_msg_dispatch_jobs_read_client_id ON msg_dispatch_jobs_read (client_id);
CREATE INDEX IF NOT EXISTS idx_msg_dispatch_jobs_read_application ON msg_dispatch_jobs_read (application);
CREATE INDEX IF NOT EXISTS idx_msg_dispatch_jobs_read_subscription_id ON msg_dispatch_jobs_read (subscription_id);
CREATE INDEX IF NOT EXISTS idx_msg_dispatch_jobs_read_message_group ON msg_dispatch_jobs_read (message_group);
CREATE INDEX IF NOT EXISTS idx_msg_dispatch_jobs_read_created_at ON msg_dispatch_jobs_read (created_at);

-- =============================================================================
-- msg_dispatch_job_attempts - Delivery attempt history
-- =============================================================================
CREATE TABLE IF NOT EXISTS msg_dispatch_job_attempts (
    id VARCHAR(17) PRIMARY KEY,
    dispatch_job_id VARCHAR(17) NOT NULL,
    attempt_number INTEGER,
    status VARCHAR(20),
    response_code INTEGER,
    response_body TEXT,
    error_message TEXT,
    error_stack_trace TEXT,
    error_type VARCHAR(20),
    duration_millis BIGINT,
    attempted_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_msg_dispatch_job_attempts_job_number ON msg_dispatch_job_attempts (dispatch_job_id, attempt_number);
CREATE INDEX IF NOT EXISTS idx_msg_dispatch_job_attempts_job ON msg_dispatch_job_attempts (dispatch_job_id);

-- =============================================================================
-- msg_event_types - Event type definitions
-- =============================================================================
CREATE TABLE IF NOT EXISTS msg_event_types (
    id VARCHAR(17) PRIMARY KEY,
    code VARCHAR(255) NOT NULL UNIQUE,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    status VARCHAR(20) NOT NULL DEFAULT 'CURRENT',
    source VARCHAR(20) NOT NULL DEFAULT 'UI',
    client_scoped BOOLEAN NOT NULL DEFAULT FALSE,
    application VARCHAR(100) NOT NULL,
    subdomain VARCHAR(100) NOT NULL,
    aggregate VARCHAR(100) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_msg_event_types_code ON msg_event_types (code);
CREATE INDEX IF NOT EXISTS idx_msg_event_types_status ON msg_event_types (status);
CREATE INDEX IF NOT EXISTS idx_msg_event_types_source ON msg_event_types (source);
CREATE INDEX IF NOT EXISTS idx_msg_event_types_application ON msg_event_types (application);
CREATE INDEX IF NOT EXISTS idx_msg_event_types_subdomain ON msg_event_types (subdomain);
CREATE INDEX IF NOT EXISTS idx_msg_event_types_aggregate ON msg_event_types (aggregate);

-- =============================================================================
-- msg_event_type_spec_versions - Event type schema versions
-- =============================================================================
CREATE TABLE IF NOT EXISTS msg_event_type_spec_versions (
    id VARCHAR(17) PRIMARY KEY,
    event_type_id VARCHAR(17) NOT NULL,
    version VARCHAR(20) NOT NULL,
    mime_type VARCHAR(100) NOT NULL,
    schema_content JSONB,
    schema_type VARCHAR(20) NOT NULL,
    status VARCHAR(20) NOT NULL DEFAULT 'FINALISING',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_msg_spec_versions_event_type ON msg_event_type_spec_versions (event_type_id);
CREATE INDEX IF NOT EXISTS idx_msg_spec_versions_status ON msg_event_type_spec_versions (status);
CREATE UNIQUE INDEX IF NOT EXISTS uq_msg_spec_versions_event_type_version ON msg_event_type_spec_versions (event_type_id, version);

-- =============================================================================
-- msg_subscriptions - Event subscriptions to endpoints
-- =============================================================================
CREATE TABLE IF NOT EXISTS msg_subscriptions (
    id VARCHAR(17) PRIMARY KEY,
    code VARCHAR(100) NOT NULL,
    application_code VARCHAR(100),
    name VARCHAR(255) NOT NULL,
    description TEXT,
    client_id VARCHAR(17),
    client_identifier VARCHAR(100),
    client_scoped BOOLEAN NOT NULL DEFAULT FALSE,
    target VARCHAR(500) NOT NULL,
    queue VARCHAR(255),
    source VARCHAR(20) NOT NULL DEFAULT 'UI',
    status VARCHAR(20) NOT NULL DEFAULT 'ACTIVE',
    max_age_seconds INTEGER NOT NULL DEFAULT 86400,
    dispatch_pool_id VARCHAR(17),
    dispatch_pool_code VARCHAR(100),
    delay_seconds INTEGER NOT NULL DEFAULT 0,
    sequence INTEGER NOT NULL DEFAULT 99,
    mode VARCHAR(20) NOT NULL DEFAULT 'IMMEDIATE',
    timeout_seconds INTEGER NOT NULL DEFAULT 30,
    max_retries INTEGER NOT NULL DEFAULT 3,
    service_account_id VARCHAR(17),
    data_only BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_msg_subscriptions_code_client ON msg_subscriptions (code, client_id);
CREATE INDEX IF NOT EXISTS idx_msg_subscriptions_status ON msg_subscriptions (status);
CREATE INDEX IF NOT EXISTS idx_msg_subscriptions_client_id ON msg_subscriptions (client_id);
CREATE INDEX IF NOT EXISTS idx_msg_subscriptions_source ON msg_subscriptions (source);
CREATE INDEX IF NOT EXISTS idx_msg_subscriptions_dispatch_pool ON msg_subscriptions (dispatch_pool_id);

-- =============================================================================
-- msg_subscription_event_types - Subscription event type bindings (junction)
-- =============================================================================
CREATE TABLE IF NOT EXISTS msg_subscription_event_types (
    id SERIAL PRIMARY KEY,
    subscription_id VARCHAR(17) NOT NULL,
    event_type_id VARCHAR(17),
    event_type_code VARCHAR(255) NOT NULL,
    spec_version VARCHAR(50)
);

CREATE INDEX IF NOT EXISTS idx_msg_sub_event_types_subscription ON msg_subscription_event_types (subscription_id);
CREATE INDEX IF NOT EXISTS idx_msg_sub_event_types_event_type ON msg_subscription_event_types (event_type_id);

-- =============================================================================
-- msg_subscription_custom_configs - Subscription custom configuration (junction)
-- =============================================================================
CREATE TABLE IF NOT EXISTS msg_subscription_custom_configs (
    id SERIAL PRIMARY KEY,
    subscription_id VARCHAR(17) NOT NULL,
    config_key VARCHAR(100) NOT NULL,
    config_value VARCHAR(1000) NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_msg_sub_configs_subscription ON msg_subscription_custom_configs (subscription_id);

-- =============================================================================
-- msg_dispatch_pools - Dispatch pool rate limiting
-- =============================================================================
CREATE TABLE IF NOT EXISTS msg_dispatch_pools (
    id VARCHAR(17) PRIMARY KEY,
    code VARCHAR(100) NOT NULL,
    name VARCHAR(255) NOT NULL,
    description VARCHAR(500),
    rate_limit INTEGER NOT NULL DEFAULT 100,
    concurrency INTEGER NOT NULL DEFAULT 10,
    client_id VARCHAR(17),
    client_identifier VARCHAR(100),
    status VARCHAR(20) NOT NULL DEFAULT 'ACTIVE',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_msg_dispatch_pools_code_client ON msg_dispatch_pools (code, client_id);
CREATE INDEX IF NOT EXISTS idx_msg_dispatch_pools_status ON msg_dispatch_pools (status);
CREATE INDEX IF NOT EXISTS idx_msg_dispatch_pools_client_id ON msg_dispatch_pools (client_id);

-- =============================================================================
-- msg_connections - Named endpoint + credential groupings
-- =============================================================================
CREATE TABLE IF NOT EXISTS msg_connections (
    id VARCHAR(17) PRIMARY KEY,
    code VARCHAR(100) NOT NULL,
    name VARCHAR(255) NOT NULL,
    description VARCHAR(500),
    endpoint VARCHAR(500) NOT NULL,
    external_id VARCHAR(100),
    status VARCHAR(20) NOT NULL DEFAULT 'ACTIVE',
    service_account_id VARCHAR(17) NOT NULL,
    client_id VARCHAR(17),
    client_identifier VARCHAR(100),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_msg_connections_code_client ON msg_connections (code, client_id);
CREATE INDEX IF NOT EXISTS idx_msg_connections_status ON msg_connections (status);
CREATE INDEX IF NOT EXISTS idx_msg_connections_client_id ON msg_connections (client_id);
CREATE INDEX IF NOT EXISTS idx_msg_connections_service_account ON msg_connections (service_account_id);
