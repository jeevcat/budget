-- Migrate all UUID columns from TEXT to native PostgreSQL UUID type.
--
-- Strategy: drop FK constraints, convert all columns, re-add FKs.
-- This avoids type-mismatch errors during conversion.

-- 1. Drop all foreign key constraints that reference UUID columns.
--    PostgreSQL auto-names inline REFERENCES as <table>_<col>_fkey.
ALTER TABLE categories DROP CONSTRAINT IF EXISTS categories_parent_id_fkey;
ALTER TABLE accounts DROP CONSTRAINT IF EXISTS accounts_connection_id_fkey;
ALTER TABLE transactions DROP CONSTRAINT IF EXISTS transactions_account_id_fkey;
ALTER TABLE transactions DROP CONSTRAINT IF EXISTS transactions_category_id_fkey;
ALTER TABLE rules DROP CONSTRAINT IF EXISTS rules_target_category_id_fkey;
ALTER TABLE schedule_runs DROP CONSTRAINT IF EXISTS schedule_runs_account_id_fkey;

-- 1b. Drop indexes that use COALESCE on UUID columns (TEXT-incompatible after conversion).
DROP INDEX IF EXISTS idx_categories_unique_name_parent;

-- 2. Convert all TEXT UUID columns to native UUID type.

-- categories
ALTER TABLE categories ALTER COLUMN id TYPE UUID USING id::uuid;
ALTER TABLE categories ALTER COLUMN parent_id TYPE UUID USING parent_id::uuid;

-- connections
ALTER TABLE connections ALTER COLUMN id TYPE UUID USING id::uuid;

-- accounts
ALTER TABLE accounts ALTER COLUMN id TYPE UUID USING id::uuid;
ALTER TABLE accounts ALTER COLUMN connection_id TYPE UUID USING connection_id::uuid;

-- transactions
ALTER TABLE transactions ALTER COLUMN id TYPE UUID USING id::uuid;
ALTER TABLE transactions ALTER COLUMN account_id TYPE UUID USING account_id::uuid;
ALTER TABLE transactions ALTER COLUMN category_id TYPE UUID USING category_id::uuid;
ALTER TABLE transactions ALTER COLUMN correlation_id TYPE UUID USING correlation_id::uuid;

-- rules
ALTER TABLE rules ALTER COLUMN id TYPE UUID USING id::uuid;
ALTER TABLE rules ALTER COLUMN target_category_id TYPE UUID USING target_category_id::uuid;

-- schedule_runs
ALTER TABLE schedule_runs ALTER COLUMN id TYPE UUID USING id::uuid;
ALTER TABLE schedule_runs ALTER COLUMN account_id TYPE UUID USING account_id::uuid;

-- 3. Re-add foreign key constraints.
ALTER TABLE categories ADD CONSTRAINT categories_parent_id_fkey
    FOREIGN KEY (parent_id) REFERENCES categories(id);

ALTER TABLE accounts ADD CONSTRAINT accounts_connection_id_fkey
    FOREIGN KEY (connection_id) REFERENCES connections(id);

ALTER TABLE transactions ADD CONSTRAINT transactions_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id);

ALTER TABLE transactions ADD CONSTRAINT transactions_category_id_fkey
    FOREIGN KEY (category_id) REFERENCES categories(id);

ALTER TABLE rules ADD CONSTRAINT rules_target_category_id_fkey
    FOREIGN KEY (target_category_id) REFERENCES categories(id);

ALTER TABLE schedule_runs ADD CONSTRAINT schedule_runs_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id);

-- 4. Recreate indexes that depended on TEXT UUID columns.
CREATE UNIQUE INDEX idx_categories_unique_name_parent
    ON categories (name, COALESCE(parent_id, '00000000-0000-0000-0000-000000000000'::uuid));
