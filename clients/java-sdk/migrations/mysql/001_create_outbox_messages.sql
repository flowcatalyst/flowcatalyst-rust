CREATE TABLE outbox_messages (
    -- Columns required by the Java outbox-processor
    id VARCHAR(26) PRIMARY KEY,
    type VARCHAR(20) NOT NULL,
    message_group VARCHAR(255),
    payload LONGTEXT NOT NULL,
    status SMALLINT NOT NULL DEFAULT 0,
    retry_count SMALLINT NOT NULL DEFAULT 0,
    created_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3),
    updated_at DATETIME(3) NOT NULL DEFAULT CURRENT_TIMESTAMP(3) ON UPDATE CURRENT_TIMESTAMP(3),
    error_message TEXT,

    -- SDK-specific columns (ignored by the processor)
    client_id VARCHAR(26),
    payload_size BIGINT,
    headers JSON,

    -- Indexes matching processor expectations
    INDEX idx_outbox_messages_pending (status, message_group, created_at),
    INDEX idx_outbox_messages_stuck (status, created_at),

    -- SDK-specific index
    INDEX idx_outbox_client_pending (client_id, status, created_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
