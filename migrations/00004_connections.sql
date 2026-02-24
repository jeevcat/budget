CREATE TABLE connections (
    id TEXT NOT NULL PRIMARY KEY,
    provider TEXT NOT NULL,
    provider_session_id TEXT NOT NULL,
    institution_name TEXT NOT NULL,
    valid_until TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE state_tokens (
    token TEXT NOT NULL PRIMARY KEY,
    user_data TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    used INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

ALTER TABLE accounts ADD COLUMN connection_id TEXT REFERENCES connections(id);

CREATE INDEX idx_connections_status ON connections(status);
CREATE INDEX idx_state_tokens_expires_at ON state_tokens(expires_at);
CREATE INDEX idx_accounts_connection_id ON accounts(connection_id);
