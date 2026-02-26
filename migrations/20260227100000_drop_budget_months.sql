ALTER TABLE transactions DROP COLUMN IF EXISTS budget_month_id;
DROP INDEX IF EXISTS idx_transactions_budget_month_id;
DROP TABLE IF EXISTS budget_months;
