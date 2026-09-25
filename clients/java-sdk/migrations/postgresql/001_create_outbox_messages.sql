CREATE TABLE outbox_messages (
    -- Columns required by the Java outbox-processor
    id VARCHAR(26) PRIMARY KEY,
    type VARCHAR(20) NOT NULL,
    message_group VARCHAR(255),
    payload TEXT NOT NULL,
    status SMALLINT NOT NULL DEFAULT 0,
    retry_count SMALLINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    error_message TEXT,

    -- SDK-specific columns (ignored by the processor)
    client_id VARCHAR(26),
    payload_size INTEGER,
    headers JSONB
);

-- Partial index for fetching pending items (matches processor expectation)
CREATE INDEX idx_outbox_messages_pending
    ON outbox_messages(status, message_group, created_at)
    WHERE status = 0;

-- Partial index for crash recovery (stuck in-progress items)
CREATE INDEX idx_outbox_messages_stuck
    ON outbox_messages(status, created_at)
    WHERE status = 9;

-- SDK-specific: polling by client
CREATE INDEX idx_outbox_client_pending
    ON outbox_messages(client_id, status, created_at);
