-- Budget domain schema (PostgreSQL native types)

CREATE TABLE IF NOT EXISTS _health_check (id INTEGER PRIMARY KEY);

CREATE TABLE IF NOT EXISTS categories (
    id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    parent_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (parent_id) REFERENCES categories(id)
);

CREATE TABLE IF NOT EXISTS connections (
    id TEXT NOT NULL PRIMARY KEY,
    provider TEXT NOT NULL,
    provider_session_id TEXT NOT NULL,
    institution_name TEXT NOT NULL,
    valid_until TIMESTAMPTZ NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS accounts (
    id TEXT NOT NULL PRIMARY KEY,
    provider_account_id TEXT NOT NULL,
    name TEXT NOT NULL,
    institution TEXT NOT NULL,
    account_type TEXT NOT NULL,
    currency TEXT NOT NULL,
    connection_id TEXT REFERENCES connections(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS budget_months (
    id TEXT NOT NULL PRIMARY KEY,
    start_date DATE NOT NULL,
    end_date DATE,
    salary_transactions_detected INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS projects (
    id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    category_id TEXT NOT NULL,
    start_date DATE NOT NULL,
    end_date DATE,
    budget_amount NUMERIC,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (category_id) REFERENCES categories(id)
);

CREATE TABLE IF NOT EXISTS transactions (
    id TEXT NOT NULL PRIMARY KEY,
    account_id TEXT NOT NULL,
    category_id TEXT,
    amount NUMERIC NOT NULL,
    original_amount NUMERIC,
    original_currency TEXT,
    merchant_name TEXT NOT NULL,
    description TEXT NOT NULL,
    posted_date DATE NOT NULL,
    budget_month_id TEXT,
    project_id TEXT,
    correlation_id TEXT,
    correlation_type TEXT,
    provider_transaction_id TEXT,
    suggested_category TEXT,
    category_method TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (account_id) REFERENCES accounts(id),
    FOREIGN KEY (category_id) REFERENCES categories(id),
    FOREIGN KEY (budget_month_id) REFERENCES budget_months(id),
    FOREIGN KEY (project_id) REFERENCES projects(id)
);

CREATE TABLE IF NOT EXISTS rules (
    id TEXT NOT NULL PRIMARY KEY,
    rule_type TEXT NOT NULL,
    match_field TEXT NOT NULL,
    match_pattern TEXT NOT NULL,
    target_category_id TEXT,
    target_correlation_type TEXT,
    priority INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (target_category_id) REFERENCES categories(id)
);

CREATE TABLE IF NOT EXISTS budget_periods (
    id TEXT NOT NULL PRIMARY KEY,
    category_id TEXT NOT NULL,
    period_type TEXT NOT NULL,
    amount NUMERIC NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (category_id) REFERENCES categories(id)
);

CREATE TABLE IF NOT EXISTS state_tokens (
    token TEXT NOT NULL PRIMARY KEY,
    user_data TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    used INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_transactions_account_id ON transactions(account_id);
CREATE INDEX IF NOT EXISTS idx_transactions_category_id ON transactions(category_id);
CREATE INDEX IF NOT EXISTS idx_transactions_posted_date ON transactions(posted_date);
CREATE INDEX IF NOT EXISTS idx_transactions_budget_month_id ON transactions(budget_month_id);
CREATE INDEX IF NOT EXISTS idx_transactions_project_id ON transactions(project_id);
CREATE INDEX IF NOT EXISTS idx_transactions_correlation_id ON transactions(correlation_id);
CREATE INDEX IF NOT EXISTS idx_categories_parent_id ON categories(parent_id);
CREATE INDEX IF NOT EXISTS idx_budget_periods_category_id ON budget_periods(category_id);
CREATE INDEX IF NOT EXISTS idx_rules_rule_type ON rules(rule_type);
CREATE INDEX IF NOT EXISTS idx_projects_category_id ON projects(category_id);
CREATE INDEX IF NOT EXISTS idx_connections_status ON connections(status);
CREATE INDEX IF NOT EXISTS idx_state_tokens_expires_at ON state_tokens(expires_at);
CREATE INDEX IF NOT EXISTS idx_accounts_connection_id ON accounts(connection_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_transactions_provider_dedup ON transactions(account_id, provider_transaction_id);
CREATE INDEX IF NOT EXISTS idx_transactions_suggested_category ON transactions(suggested_category) WHERE suggested_category IS NOT NULL;
