-- Store the LLM's proposed category name on transactions so users can
-- review a histogram of suggestions and create categories from them.

ALTER TABLE transactions ADD COLUMN suggested_category TEXT;

CREATE INDEX IF NOT EXISTS idx_transactions_suggested_category
    ON transactions(suggested_category)
    WHERE suggested_category IS NOT NULL;
