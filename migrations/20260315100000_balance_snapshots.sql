CREATE TABLE IF NOT EXISTS balance_snapshots (
    id UUID NOT NULL PRIMARY KEY,
    account_id UUID NOT NULL REFERENCES accounts(id),
    current_balance NUMERIC NOT NULL,
    available_balance NUMERIC,
    currency TEXT NOT NULL,
    snapshot_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_balance_snapshots_account_id ON balance_snapshots(account_id);
CREATE INDEX IF NOT EXISTS idx_balance_snapshots_account_time ON balance_snapshots(account_id, snapshot_at DESC);
