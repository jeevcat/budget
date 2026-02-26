-- Collapse budget_periods and projects into categories.
-- budget_mode becomes a first-class property of a category.

ALTER TABLE categories ADD COLUMN budget_mode TEXT;
ALTER TABLE categories ADD COLUMN budget_amount NUMERIC;
ALTER TABLE categories ADD COLUMN project_start_date DATE;
ALTER TABLE categories ADD COLUMN project_end_date DATE;

-- Migrate budget periods into categories
UPDATE categories SET
    budget_mode = bp.period_type,
    budget_amount = bp.amount
FROM budget_periods bp
WHERE bp.category_id = categories.id;

-- Migrate projects into categories
UPDATE categories SET
    budget_mode = 'project',
    budget_amount = p.budget_amount,
    project_start_date = p.start_date,
    project_end_date = p.end_date
FROM projects p
WHERE p.category_id = categories.id;

-- Drop project_id from transactions
ALTER TABLE transactions DROP COLUMN project_id;

-- Drop indexes before dropping tables
DROP INDEX IF EXISTS idx_projects_category_id;
DROP INDEX IF EXISTS idx_budget_periods_category_id;
DROP INDEX IF EXISTS idx_transactions_project_id;

-- Drop old tables
DROP TABLE budget_periods;
DROP TABLE projects;
