-- Domain schema for budget application

CREATE TABLE IF NOT EXISTS accounts (
    id TEXT NOT NULL PRIMARY KEY,
    provider_account_id TEXT NOT NULL,
    name TEXT NOT NULL,
    institution TEXT NOT NULL,
    account_type TEXT NOT NULL,
    currency TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP),
    updated_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP)
);

CREATE TABLE IF NOT EXISTS categories (
    id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    parent_id TEXT,
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP),
    FOREIGN KEY (parent_id) REFERENCES categories(id)
);

CREATE TABLE IF NOT EXISTS budget_months (
    id TEXT NOT NULL PRIMARY KEY,
    start_date TEXT NOT NULL,
    end_date TEXT,
    salary_transactions_detected INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP)
);

CREATE TABLE IF NOT EXISTS projects (
    id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    category_id TEXT NOT NULL,
    start_date TEXT NOT NULL,
    end_date TEXT,
    budget_amount TEXT,
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP),
    FOREIGN KEY (category_id) REFERENCES categories(id)
);

CREATE TABLE IF NOT EXISTS transactions (
    id TEXT NOT NULL PRIMARY KEY,
    account_id TEXT NOT NULL,
    category_id TEXT,
    amount TEXT NOT NULL,
    original_amount TEXT,
    original_currency TEXT,
    merchant_name TEXT NOT NULL,
    description TEXT NOT NULL,
    posted_date TEXT NOT NULL,
    budget_month_id TEXT,
    project_id TEXT,
    correlation_id TEXT,
    correlation_type TEXT,
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP),
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
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP),
    FOREIGN KEY (target_category_id) REFERENCES categories(id)
);

CREATE TABLE IF NOT EXISTS budget_periods (
    id TEXT NOT NULL PRIMARY KEY,
    category_id TEXT NOT NULL,
    period_type TEXT NOT NULL,
    amount TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP),
    FOREIGN KEY (category_id) REFERENCES categories(id)
);

-- Indexes for common query patterns
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
