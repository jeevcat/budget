-- Unify schedule_runs for both bank and Amazon accounts.
-- Drop the FK to accounts(id) so Amazon account UUIDs can be tracked too.
ALTER TABLE schedule_runs DROP CONSTRAINT schedule_runs_account_id_fkey;
ALTER TABLE schedule_runs ADD COLUMN account_type TEXT NOT NULL DEFAULT 'bank';
CREATE INDEX idx_schedule_runs_type_account ON schedule_runs(account_type, account_id);
