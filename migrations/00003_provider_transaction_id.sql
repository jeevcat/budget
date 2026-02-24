ALTER TABLE transactions ADD COLUMN provider_transaction_id TEXT;

CREATE UNIQUE INDEX idx_transactions_provider_dedup
    ON transactions(account_id, provider_transaction_id);
