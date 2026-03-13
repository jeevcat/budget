-- Multi-account support for Amazon enrichment

CREATE TABLE amazon_accounts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    label TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Scope transactions to accounts
ALTER TABLE amazon_transactions
    ADD COLUMN amazon_account_id UUID REFERENCES amazon_accounts(id) ON DELETE CASCADE;
CREATE INDEX idx_amazon_transactions_account ON amazon_transactions (amazon_account_id);

-- Dedup key must be unique per account, not globally
ALTER TABLE amazon_transactions
    DROP CONSTRAINT amazon_transactions_dedup_key_key;
ALTER TABLE amazon_transactions
    ADD CONSTRAINT amazon_transactions_account_dedup UNIQUE (amazon_account_id, dedup_key);
