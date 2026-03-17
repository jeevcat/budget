# TODO

## API Design

- [ ] **Cursor-based pagination**: Migrate `/transactions` from offset-based (`limit`/`offset`/`total`) to cursor-based pagination (`cursor`/`limit` request, `next_cursor` response). Update `TransactionPage`, the DB query, and the frontend

## Bugs

- [x] **Net worth card NaN on dashboard**: Dashboard net worth card reads `a.balance` but API returns `a.current` → `Number(undefined)` = NaN for every account amount and color conditional. Fixed: `a.balance` → `a.current` on line ~1384 of `app.js`

## Navigation & Information Architecture Refactor

Goal: restructure 8 flat nav items into a clear hierarchy that separates daily-use pages from admin/config pages, and fix naming to match what each page actually does.

### Context from SPEC.md

The spec (§10 Insights & Forecasting) plans significant expansion of the Insights page: spending anomaly detection (BOCPD changepoints + MSTL outlier flagging), seasonality-aware pacing, trend warnings ("Groceries trending up €24/year"), and budget adjustment suggestions. Anomaly badges on dashboard rows will link to detail on Insights. So Insights must remain a dedicated top-level page — it's the future home for all budget intelligence, not just burndown charts.

The spec also says the dashboard net worth card is intentional (§ Balances Page: "The Dashboard shows a net worth card linking to the Balances page"). Keep it but fix the field name bug (done above).

### Rename pages

- [ ] **Rename Dashboard → Budget**: The page is a budget tracker (monthly/annual/projects ledger with pace indicators), not a general dashboard. Routes stay the same (`/`, `/monthly`, `/annual`, `/projects`). Change the nav link text, page heading if one exists, and any references in E2E tests
- [ ] **Rename Balances → Net Worth**: The page shows total net worth, per-account breakdown, projection chart with confidence bands, and balance history. "Balances" undersells it. Change the nav link, `<h2>`, route from `#/balances` to `#/net-worth`, and the net worth card's `href` on the budget page. Update E2E test section headers

### Restructure sidebar navigation

Current sidebar: Dashboard, Transactions, Insights, Balances, Categories, Rules, Connections, Jobs (8 flat items, equal visual weight).

Proposed sidebar — split into two groups with a visual divider:

```
Budget           ← daily use (renamed from Dashboard)
Transactions     ← daily use
Insights         ← daily use (will grow with anomaly detection)
Net Worth        ← daily use (renamed from Balances)
─────────        ← visual divider (thin border-top or extra margin)
Categories       ← config/admin
Rules            ← config/admin
Connections      ← config/admin
Jobs             ← config/admin
```

Implementation:
- [ ] **Add sidebar divider**: Insert a visual separator between Net Worth and Categories nav links. Use Oat utility or a thin `<hr>` styled with `border-color: var(--border); opacity: 0.3; margin: 0.5rem 0`. No custom CSS class needed
- [ ] **Reorder nav links**: Move nav items to match the order above. Currently Insights is 3rd and Balances is 4th — they stay in place, just the rename applies. Categories/Rules/Connections/Jobs stay in current order below the divider

### Burndown drill-down from budget page

The spec says (§ Budget Burndown Charts): "Access: Drill-down from a variable category's pace indicator on the dashboard to the burndown chart on the insights page." This is not yet implemented — clicking a category on the budget page filters the transaction list but doesn't link to the burndown.

- [ ] **Category drill-down to burndown**: When a user clicks a variable monthly category on the Budget page, navigate to `#/insights/{categoryId}`. The Insights page already accepts `categoryId` as a route param and pre-selects it. This makes the burndown chart feel like a natural extension of the budget rather than a separate disconnected page. Consider: add this as a secondary action (e.g., small chart icon on the row) rather than replacing the current click-to-filter behavior, since filtering the transaction list is also useful

### Future consideration: Insights page expansion

When anomaly detection is implemented (see "Insights & Analytics" section below), the Insights page will need:
- Category list/grid showing all categories with anomaly indicators
- Anomaly detail cards (structural shift explanation, one-off spike context)
- Trend visualization (MSTL decomposition chart: trend + seasonal + residual)
- Seasonality calendar or heatmap
- The burndown chart stays as one section among several

The current single-burndown layout should be designed to accommodate additional sections stacking vertically below. No changes needed now — just don't paint yourself into a corner with the current layout.

## From Spec — Remaining Work

### Edge Case Coverage
- [ ] **Late/missing salary UX**: Verify the previous budget month stays open indefinitely and surface a clear signal when expected salaries haven't arrived

### Frontend
- [ ] **Skip correlation button**: Add "Skip Correlation" button to transaction detail panel when a correlation is present, using `POST /transactions/{id}/skip-correlation`. Lets users fix mistaken auto-correlations
- [ ] **Amazon account rename**: Add click-to-edit label on Amazon account cards using `PATCH /amazon/accounts/{id}`. Currently requires delete + recreate
- [ ] **Amazon matches list**: Show which transactions matched which Amazon orders on the Connections page using `GET /amazon/matches`. Currently matches are only visible one-at-a-time in the transaction detail panel

## Insights & Analytics

### Features
- [ ] **Spending anomaly detection**: Changepoint detection (BOCPD) for structural shifts ("groceries shifted +35% in October") + outlier flagging on MSTL residuals for one-off spikes. Surface on dashboard as subtle badge on category rows linking to detail on insights page. Run on all categories with 3+ months of history

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

## Blocked Upstream

- [ ] **Gradle in Claude Code sandbox**: `dl.google.com` / `maven.google.com` are blocked by the sandbox egress proxy (403 `host_not_allowed`), so Gradle can't resolve AGP or Google Maven deps. The pre-commit hook gracefully skips Kotlin compilation when this happens. Once [anthropics/claude-code#16222](https://github.com/anthropics/claude-code/issues/16222) is fixed, remove the skip logic from `.github/hooks/pre-commit` and verify `./gradlew compileDebugKotlin` works in sandbox sessions
