# TODO

## API Design

- [ ] **Cursor-based pagination**: Migrate `/transactions` from offset-based (`limit`/`offset`/`total`) to cursor-based pagination (`cursor`/`limit` request, `next_cursor` response). Update `TransactionPage`, the DB query, and the frontend

## From Spec — Remaining Work

### Frontend
- [ ] **Skip correlation button**: Add "Skip Correlation" button to transaction detail panel when a correlation is present, using `POST /transactions/{id}/skip-correlation`. Lets users fix mistaken auto-correlations
- [ ] Be able to edit the auto-labels of transactions for manual correction
- [ ] Annual Budget page has weird custom section for "Monthly Budgets". Why not just instead show monthly budgets broken up by monthly category with normal monthly budget bars? So it looks the same as the annual categories. But obviously under a separate header.
- [ ] Visual hierarchy on Budget page is not great. Need to think high level on best approach. How many levels do we have? Are we using semantic h1 etc?


## E2E Tests

Current coverage: 3 smoke tests (dashboard loads, insights loads, no burndown on empty DB). All run against an empty database with no seeded data. Tests need API-seeded data (POST via `fetch` or Playwright `request`) since there's no seed script.

### Navigation & Auth
- [ ] **Nav links**: Click each sidebar link (Transactions, Insights, Net Worth, Categories, Rules, Connections, Jobs) and assert the correct page heading renders
- [ ] **Sign out**: Click sign out, verify redirect to login page, verify authenticated routes are inaccessible
- [ ] **Auth guard**: Navigate to a protected route without auth, verify login page is shown

### Categories (CRUD)
- [ ] **Create category**: Add a root category via the form, verify it appears in the category tree
- [ ] **Create subcategory**: Add a child category under an existing parent, verify hierarchy
- [ ] **Edit category**: Change name, budget mode, amount — verify updates persist after page reload
- [ ] **Delete category**: Remove a category, verify it disappears from the tree
- [ ] **Budget modes**: Create categories with each budget mode (monthly/annual/project) and verify the correct fields appear (amount, date pickers for project)

### Rules (CRUD + Apply)
- [ ] **Create categorization rule**: Add a rule with a merchant condition + category target, verify it appears in the rules table
- [ ] **Create correlation rule**: Add a correlation rule (transfer/reimbursement type), verify display
- [ ] **Edit rule**: Modify conditions/target/priority, verify updates
- [ ] **Delete rule**: Remove a rule, verify it disappears
- [ ] **Rule preview**: Create a rule, use the preview button, verify match count and sample merchants display
- [ ] **Apply all rules**: Seed uncategorized transactions, create a matching rule, apply all, verify transactions get categorized

### Transactions
- [ ] **List renders**: Seed transactions via API, navigate to `/transactions`, verify table rows appear
- [ ] **Search**: Type in search box, verify table filters to matching merchants
- [ ] **Category filter**: Select a category from dropdown, verify only matching transactions show
- [ ] **Account filter**: Select an account, verify filtering works
- [ ] **Method filter**: Filter by categorization method (Manual/Rule/LLM/Uncategorized)
- [ ] **Pagination**: Seed >50 transactions, verify page controls work (next/prev, page count)
- [ ] **Sort by date**: Click date column header, verify sort order toggles
- [ ] **Detail panel open/close**: Click a transaction row, verify slide-in panel opens with correct data; click close or backdrop to dismiss
- [ ] **Manual categorize**: Open detail panel, select a category, verify it saves and method shows "Manual"
- [ ] **Clear category**: Categorize a transaction, then clear it, verify it becomes uncategorized

### Budget (Dashboard)
- [ ] **Monthly tab with data**: Seed categories with budgets + transactions, verify ledger renders income, variable, fixed, and net summary
- [ ] **Month navigation**: Click next/prev month buttons, verify heading and data update
- [ ] **Variable category progress bars**: Verify bars render with correct pace coloring
- [ ] **Fixed category status icons**: Verify pending/check icons display correctly
- [ ] **Zero-spend chips**: Categories with budget but no spending show as compact chips
- [ ] **Unbudgeted section**: Seed a transaction with no budget category, verify it appears under unbudgeted
- [ ] **Tab switching**: Click Annual and Projects tabs, verify content changes
- [ ] **Transaction row click**: Click a transaction in the dashboard table, verify detail panel opens

### Insights
- [ ] **Burndown chart with data**: Seed a monthly variable category with transactions, verify burndown card appears with chart SVG, stats (budget, spent, predicted)
- [ ] **Burndown category selector**: Switch between categories in the dropdown, verify chart updates

### Net Worth (Balances)
- [ ] **Net worth summary**: Seed account balances, verify total net worth and per-account breakdown render
- [ ] **Net worth chart with data**: Seed account balances, verify projection card renders with chart SVG and stats
- [ ] **Balance history**: Seed multiple balance snapshots for an account, verify expandable history shows snapshots
- [ ] **Manual balance recording**: Create a manual account, record a balance, verify it appears in history

### Connections & Accounts
- [ ] **Create manual account**: Fill form (name, institution, type, currency), submit, verify account appears in the list
- [ ] **Edit account nickname**: Click account name, edit inline, verify update persists
- [ ] **CSV import**: Create a manual account, upload a CSV file, verify import count feedback
- [ ] **Bank search**: Enter a country code, click search (expect empty result or sandbox response)

### Jobs
- [ ] **Page loads with queue cards**: Verify Sync, Categorize, Correlate, Amazon cards render
- [ ] **Trigger categorize**: Click the Categorize trigger button, verify job starts (status updates)
- [ ] **Trigger correlate**: Click the Correlate trigger button, verify response
- [ ] **Job counts update**: Trigger a job, verify active/waiting badges update via polling

