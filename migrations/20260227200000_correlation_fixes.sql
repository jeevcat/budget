-- Fix correlation system: enforce 1:1 pairing, add skip flag, add date proximity index.

-- Clear duplicate correlation_id values that violate the unique constraint.
-- Each correlation_id should appear on exactly one row (A points to B, B points to A).
UPDATE transactions SET correlation_id = NULL, correlation_type = NULL
WHERE correlation_id IN (
    SELECT correlation_id FROM transactions
    WHERE correlation_id IS NOT NULL
    GROUP BY correlation_id HAVING COUNT(*) > 1
);

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
