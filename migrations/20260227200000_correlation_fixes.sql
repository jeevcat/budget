-- Fix correlation system: enforce 1:1 pairing, add skip flag, add date proximity index.

-- Replace plain index with partial UNIQUE index to enforce one-to-one pairing
DROP INDEX IF EXISTS idx_transactions_correlation_id;
CREATE UNIQUE INDEX idx_transactions_correlation_id
    ON transactions(correlation_id)
    WHERE correlation_id IS NOT NULL;

-- Allow marking transactions that should never enter correlation analysis
ALTER TABLE transactions ADD COLUMN skip_correlation BOOLEAN NOT NULL DEFAULT FALSE;

-- Speed up candidate queries that filter by amount + date range
CREATE INDEX idx_transactions_correlation_candidates
    ON transactions(amount, posted_date);
