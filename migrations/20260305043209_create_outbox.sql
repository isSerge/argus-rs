CREATE TABLE outbox (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    action_name TEXT NOT NULL,
    payload TEXT NOT NULL, -- JSON serialized ActionPayload
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    retries INTEGER NOT NULL DEFAULT 0,
    last_attempt DATETIME
);

-- Index for fast FIFO retrieval
CREATE INDEX idx_outbox_created_at ON outbox(created_at);
