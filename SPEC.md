# Personal Budgeting Tool — Product Spec

## Problem Statement

Existing budgeting tools fail because they impose rigid categorization, use calendar months that don't align with income timing, and lack intelligent handling of edge cases like large one-off expenses or cross-account transactions.

## Core Principles

- **Salary-anchored months**: A "budget month" begins only after all predefined salary/income transactions have landed, not on the 1st of the calendar month.
- **Deterministic-first categorization**: Deterministic rules (user overrides, merchant mappings, regex) always win. An LLM handles unmatched transactions but refuses to guess — low-confidence results go to the user, not into the budget.
- **Daily overspend awareness**: Real-time (daily) tracking of spend vs. budget per category for the current budget month.
- **Project budgets**: Time-bound budgets for large efforts (renovations, weddings) that live outside the monthly/annual cycle and don't distort regular budget tracking.
- **Cross-account correlation**: Link related transactions across bank and credit card accounts (e.g., credit card payment from checking, reimbursements, transfers) to avoid double-counting or misrepresentation.
- **Intelligence baked in, not bolted on**: Forecasting, anomaly detection, and seasonality feed into the core budget UI (pace indicators, warnings, suggestions) rather than living on a separate analytics page.

---

## Feature Breakdown

### 1. Account Connectivity

Connect to real bank and credit card accounts via a **provider-abstracted interface**. The abstraction exposes a minimal contract: list accounts, fetch transactions since date, get balances. The underlying provider (e.g., Enable Banking) is a config choice and can be swapped without affecting the rest of the system.

Pull transactions automatically on a regular cadence via a **job queue system**.

**Jobs are atomic steps in a chain**, each independently retriable and observable:
1. **Sync** — fetch new transactions from provider, store balance snapshots
2. **Categorize** — run deterministic rules + LLM on uncategorized transactions
3. **Correlate** — match transfers/reimbursements using deterministic rules + LLM
4. **Budget recompute** — recalculate all budget views from current transaction state

Each job triggers the next on success. If a step fails, it can be retried independently without re-running prior steps.

**Job queue is user-visible**: The system surfaces job status for each step (last run, success/failure, pending). Manual kick-off buttons allow the user to trigger any individual step on demand.

Support multiple accounts per user.

**Unified view**: The main UI shows all transactions merged across all accounts. Account is metadata on each transaction, not an organizing principle. There are no per-account views or tabs in the primary interface.

#### Manual Accounts

Not all accounts can be connected to a bank provider. Investment accounts, pensions, depository accounts, and foreign accounts may require manual tracking. Manual accounts support:

- **CSV import**: Upload transaction history (currently supports Amex German-locale CSV format). Transactions are deduplicated on `(account_id, provider_transaction_id)`.
- **Manual balance snapshots**: For accounts where transaction-level data isn't available or relevant (e.g., a brokerage account), the user can record "this account is worth €X today." These snapshots feed into net worth tracking without requiring individual transactions.

Manual accounts are created via the API with `origin: AccountOrigin::Manual` (null `connection_id`). They participate in all budget math identically to connected accounts — the only difference is how data enters the system.

#### Provider Authorization Flow

Bank aggregators (e.g., Enable Banking) require an OAuth-like redirect to establish a connection:

1. **User picks a bank**: The frontend shows a searchable list of supported banks (ASPSPs). The API exposes a search/list endpoint that queries the provider.
2. **Start authorization**: The API creates an authorization request with the provider and returns a redirect URL. The frontend opens this URL (new tab or full redirect). The API generates a random `state` token and stores it server-side to validate the callback.
3. **User authenticates at the bank**: The user logs into their bank, selects accounts, and grants consent. This happens entirely on the bank's domain.
4. **Callback**: The bank redirects back to the budget app's callback URL (`/api/connections/callback`). The API validates the `state` token, exchanges the authorization code for a session, and receives the list of accounts.
5. **Session stored**: The session ID, account list, and expiry are persisted in a `connections` table. The accounts are upserted into the `accounts` table with a foreign key to the connection.

#### Connection Persistence

A **connection** represents an authenticated session with a bank provider:
- `id` — internal UUID
- `provider` — which provider (e.g., `enable_banking`)
- `provider_session_id` — the provider's session identifier
- `institution_name` — human-readable bank name
- `valid_until` — session expiry (set at authorization time)
- `status` — `active`, `expired`, `revoked`
- `created_at`, `updated_at`

Each account in the `accounts` table has a `connection_id` foreign key (nullable — null for manual accounts). Multiple accounts can belong to one connection (e.g., checking + savings at the same bank).

#### Session Expiry and Renewal

- **PSD2 mandates a maximum consent duration of 180 days** for European bank connections via Enable Banking. Other providers may have different limits.
- The system tracks `valid_until` on each connection. A **background job** checks for connections approaching expiry (e.g., 7 days out) and flags them.
- **Renewal requires user action**: the user must re-do the bank redirect (PSD2 requires explicit re-consent). The system surfaces a "reconnect" prompt in the UI and on the dashboard. It does not silently fail — the sync job for an expired connection returns `SessionExpired`, and the UI shows a clear "connection expired, reconnect" state.
- When the user reconnects, the existing connection is updated in place (new session ID, new expiry). Account IDs from the provider are stable, so historical transactions remain linked.

#### Callback URL

- The callback URL must be reachable by the user's browser after the bank redirect. For local-network use, this is `http://<server-ip>:<port>/api/connections/callback`. For internet access via Cloudflare Tunnel, it's `https://<budget-domain>/api/connections/callback`.
- The callback URL is configured in the server config (`redirect_url`). It must match what the provider expects — mismatches will cause the redirect to fail.
- The callback endpoint is **unauthenticated** (the bank's redirect can't include our bearer token), but is protected by the `state` token: only requests with a valid, unexpired, previously-issued state token are accepted. State tokens are single-use and expire after 10 minutes.

### 2. Salary-Anchored Budget Month

- Salary detection uses the **same layered categorization system** as all other transactions (Section 3). A salary transaction is simply one that lands in a user-defined "Salary" category (or subcategories like "Salary:Employer A"). The user distinguishes salary from bonuses, tax refunds, etc. using the same deterministic rules (merchant name, amount range, regex, etc.).
- **Only monthly income triggers budget months.** Quarterly, annual, or other irregular income is categorized and tracked but does not gate the start of a budget month. These are just income transactions that happen to land at longer intervals.
- The user specifies **how many** distinct monthly salary deposits are expected per month (e.g., 2 — one from each employer).
- Budget month starts on the day the **first** of these expected monthly salary transactions posts in a given calendar month.
- **Late/missing salary**: The budget month simply does not start until all predefined salary transactions have posted. No fallback date, no partial start. The previous budget month effectively extends until the new one begins.
- **Inter-month gap**: Transactions that occur between the start of a calendar month and the day salary lands belong to the **previous budget month**. The first budget month extends backward to cover any transactions that predate the earliest salary arrival.
- **First-time backfill**: On initial account connection, historical transactions are imported and retroactively organized into budget months using the same salary-anchoring logic. This gives the user immediate visibility into past spending patterns. Because the system is always-live/functional, backfilled data is treated identically to new data — annual budgets will show cumulative spend from the start of the budget year.

### 3. Transaction Categorization

- **Layer 1 — Deterministic rules**: User-defined merchant-to-category mappings, regex/pattern rules, exact-match overrides. These always win. Rules support multiple conditions (AND logic) across fields: merchant name, description, amount range, counterparty name/IBAN/BIC, bank transaction code, and Amazon item title. Rules have priority ordering for conflict resolution.
- **Layer 2 — LLM classification**: For unmatched transactions, use a configurable LLM (model name is a config parameter) to infer category from merchant name, amount, and transaction metadata. The LLM returns a **confidence score** with every categorization.
  - **High confidence**: transaction is auto-categorized.
  - **Low confidence / no match**: transaction is **not categorized**. It goes into an **uncategorized queue** for user review. The system does not guess.
- **Rule generation from corrections**: When the user categorizes a transaction from the uncategorized queue, the LLM proposes a deterministic rule (regex, merchant match, etc.) that would catch this and similar transactions. The user confirms or edits the proposed rule, and it becomes a permanent Layer 1 rule. This makes the system self-improving — each correction reduces future uncategorized transactions.
- **Rules management is a first-class screen** — not hidden settings. It's where the user teaches the system. This is a core workflow, especially during onboarding when many transactions will be uncategorized. The screen supports creating, editing, deleting, and reviewing all deterministic rules (both categorization and correlation). Rules have a live preview showing which transactions would match.
- **One transaction, one category**: Transactions are not split across categories. Each transaction belongs to exactly one category. This keeps the model simple at the cost of some precision on mixed-purpose purchases.
- **Feedback loop**: User corrections always result in new deterministic rules (via the rule generation flow above). The LLM is never consulted for already-solved patterns.
- Categories are user-defined with sensible defaults.
- **Nested hierarchy**: Categories support parent/child nesting (e.g., Food > Groceries, Food > Restaurants). Unlimited depth, though 2–3 levels is expected in practice.
- **Independent budgets at any level**: A parent category can have its own budget independent of its children's budgets. Spend rolls up (Food shows total of all children), but budget limits are independent — being under budget on all children doesn't guarantee being under budget on the parent, and vice versa.
- **LLM configuration**: The model is a config parameter (e.g., Gemini 2.5 Flash-Lite). Calls are made per-transaction. Cost is negligible (~$0.01–0.10/month for typical usage). No batching or caching required.

### 4. Daily Overspend Monitoring

Per-category budget amounts set by the user, with support for three budget modes:
- **Monthly**: base budget amount is the same each month.
- **Annual**: cumulative spend tracked across 12 budget months. The budget year starts with the first budget month that aligns with January (i.e., after January salaries post). Consistent with the salary-anchored month logic.
- **Project**: see Section 5.

The daily overspend view shows per category: **amount spent**, **amount remaining**, and **days left in budget period** (for monthly budgets: days until next budget month starts; for annual budgets: months remaining in the budget year, since days would be too granular to be useful).

#### Pace Indicators

Budget pacing differs by category type:

- **Variable categories** (groceries, dining, entertainment): A pace indicator shows whether spend is ahead of or behind the expected rate for this point in the month. The indicator accounts for seasonal patterns — if December groceries are historically €70 above average, the pace calculation adjusts expectations accordingly rather than flagging normal seasonal spending as "over pace." Variable categories support drill-down to a **burndown chart** showing the daily cumulative spend curve, predicted end-of-month landing, and 3 previous months as ghost lines for comparison.

- **Fixed categories** (rent, insurance): A compact **paid/pending indicator** — these expenses are binary (either the expected payment has posted or it hasn't). No progress bar or pace calculation.

#### Budget Views

- **Monthly ledger**: Budgeted categories with pace indicators, monthly totals for income/expenses/savings.
- **Annual ledger**: Year-to-date spending across annual budgets.
- **Projects tab**: Project-mode categories with separate budget math (see Section 5).
- **Transaction views**: Transactions scoped to the current budget month, year-to-date, or project.

#### General Behavior

- **Uncategorized transactions**: Transactions in the uncategorized queue are included in the **overall total** as unallocated spend (so the total is never artificially low), but they do not count toward any specific category budget until categorized.
- **Always live / functional**: All views are computed from the current state of transactions. If a transaction posts late, gets recategorized, or is deleted, all daily and monthly views update retroactively. No snapshots — the system is purely functional over the current transaction set.
- **Delivery**: Dashboard, Insights page, and Balances page (pull model). User checks when they want to. Push notifications (email/mobile) are future work.
- **Overall total**: An aggregate spent/remaining across all active categories is shown for informational purposes, but overspend signals are per-category only.

### 5. Project Budgets

- A **project** is not a separate entity — it is a category (or subtree) with `budget_mode` set to `project`. The category itself carries the project's start date, optional end date, and optional budget amount. There are no project tables or linking operations; the category *is* the project.
- A project-mode category has a user-defined **start date** and an **optional end date**. Projects with no end date remain active indefinitely (useful for open-ended efforts like ongoing renovations). The budget amount is also optional. The pace indicator only displays when both an end date and a budget are set.
- Transactions are assigned to a project by being categorized into a project-mode category (or any of its children). E.g., setting "Home > Renovation" to project mode with a start date and budget captures all transactions in that category and its subtree for the project's duration.
- Project spend is tracked against its own budget and timeline, with the same spent/remaining/days-left model. `time_left` is days until the end date (or -1 for open-ended projects). The pace indicator pro-rates across the project's full timeline (start to end date).
- **Projects are excluded from monthly/annual budgets.** When computing regular budget status, transactions in project-mode categories (and their subtrees) are filtered out entirely. The "overall total" in Section 4 covers regular-budget categories only; projects have their own separate summary in the API response and a dedicated tab in the UI.
- **Ending a project**: To end a project, the user changes the category's `budget_mode` back to monthly or annual. Transactions in that category then resume flowing into regular budgets going forward. The category can later be set to project mode again for a different project (sequentially, not simultaneously).
- **Retroactive exclusion**: Because the system is always-live/functional, setting a category to project mode retroactively excludes its historical transactions from regular budget months — no reassignment needed, project membership is derived from category budget mode + date range at query time.
- Use cases: home renovations, weddings, one-time medical expenses, relocation costs, car purchases, etc.

### 6. Cross-Account Transaction Correlation

- Correlation uses the **same architecture as categorization** (Section 3): deterministic rules first, LLM fallback with confidence scores, uncategorized queue for unresolved cases, and rule generation from user corrections. However, categorization and correlation are distinct operations — categorization maps transactions to categories, while correlation links two transactions together and nets them financially.
- Correlation categories include:
  - **Transfer to [account]**: Credit card payments from checking, moves between own accounts → net zero, not an expense.
  - **Reimbursement for [transaction]**: Incoming deposit that offsets a prior expense → correlated to the original transaction, netting the category spend. The budget sees the reimbursed expense as if it never happened (computed functionally, not mutated).
- Deterministic rules handle known patterns (e.g., "CHASE CREDIT CRD" from checking → Transfer to Chase Visa). The LLM handles ambiguous cases with a confidence score. Unresolved correlations land in the **uncategorized queue** alongside uncategorized transactions, and user corrections generate permanent rules via the same feedback loop.
- **Processing order is explicit**: transactions arrive → categorize → correlate → budget math. Categorization must complete first because correlation may depend on category (e.g., knowing a charge is "Renovation" to link it to a project).

### 7. Multi-Currency Support

- User defines a single **budget currency** (e.g., EUR).
- All transactions are converted to the budget currency at the exchange rate applied at transaction time (i.e., the rate the bank/card actually charged).
- For accounts denominated in a foreign currency, conversion uses the rate on the transaction posting date.
- The **original currency and amount are stored as metadata** on each transaction for reference/display, but all budget math operates in the budget currency only.
- No equity conversion accounts, no market price revaluation, no gain/loss tracking — this is a budgeting tool, not an accounting ledger.

### 8. Amazon Order Enrichment

Amazon transactions appear on bank statements as opaque lump sums (e.g., "AMAZON.DE €47.32") with no item-level detail. The enrichment system resolves these into individual items:

- **Amazon account management**: Support multiple Amazon accounts (e.g., household members). Each account is tracked independently with its own authentication cookies.
- **Cookie-based authentication**: The user provides Amazon session cookies (JSON or Netscape cookies.txt format). The system tracks cookie expiry and prompts for renewal.
- **Order scraping**: On sync, the system fetches order history from Amazon, parsing individual orders with item-level detail (item name, subtotal, shipping, VAT, promotions).
- **Transaction matching**: Amazon orders are matched to bank transactions by amount and date proximity, with confidence scoring. Matched transactions gain item-level metadata.
- **Rule integration**: `amazon_item_title` is available as a rule condition field, enabling category rules based on what was actually purchased (e.g., "Amazon items containing 'cat food' → Pets").
- **Deduplication**: Orders are deduplicated per Amazon account to handle repeated syncs gracefully.

### 9. Authentication

- **Single user, no accounts.** The system is designed for one person. There is no registration, login, or user management.
- **Static bearer token**: The server config contains a `secret_key` (a random token, e.g., generated via `openssl rand -hex 32`). All API requests must include `Authorization: Bearer <token>`. Requests with a missing or invalid token receive a `401 Unauthorized` response.
- **Frontend auth flow**: On first visit (or when the stored token is invalid), the UI shows a simple "enter your key" screen. The token is stored in an `HttpOnly` cookie and sent automatically on subsequent requests. No session management, no expiry — the token is valid until rotated in config.
- **Health check is unauthenticated**: The `/health` endpoint does not require a token, enabling uptime monitoring from external services.
- **HTTPS is a deployment concern**: The server itself speaks plain HTTP. TLS termination is handled by a reverse proxy (e.g., Caddy, nginx with Let's Encrypt) when exposed beyond localhost. The token must not travel in the clear — HTTPS is required for any non-localhost access.
- **No user accounts, no OAuth, no sessions.** These add complexity with zero benefit for a single-user tool.

### 10. Insights & Forecasting

The system uses time series analysis to surface spending intelligence. Pace indicators become seasonality-aware, anomalies surface as badges on the dashboard, and detailed drill-downs live on a dedicated **Insights** page. The Insights page is focused on **budget intelligence** (burndown charts, anomaly detection, seasonality trends). Asset/balance visibility lives on the separate **Balances** page (see below).

**Library**: `augurs` (Rust-native time series toolkit from the Grafana community). Provides Prophet forecasting via a bundled WebAssembly Stan model (no Python sidecar), MSTL decomposition, exponential smoothing, seasonality detection, and changepoint detection — all as pure Rust dependencies.

#### Balances Page

Net worth and balance data live on a dedicated **Balances** page, separate from the budget-focused Insights page. This page is the home for all asset/liability visibility:

- **Net worth summary**: Current total net worth + per-account breakdown (name, type, latest balance, snapshot date). Uses `GET /accounts/net-worth`.
- **Net worth projection chart**: Historical net worth series + Prophet forecast with confidence bands (moved from Insights). Uses `GET /accounts/net-worth/projection`.
- **Per-account balance history**: Expandable rows showing balance snapshots over time for each account. Uses `GET /accounts/{id}/balances`.
- **Manual balance recording**: "Record Balance" button on manual accounts (no auto-sync) for hand-entered snapshots. Uses `POST /accounts/{id}/balances`. Designed for accounts where transaction-level data isn't available (investment accounts, pensions, brokerages).

The Dashboard shows a **net worth card** (current total + per-account breakdown) linking to the Balances page for the full projection and history.

##### Net Worth Projection

- **Balance snapshots**: Each bank sync stores the current account balance in a `balance_snapshots` table (account_id, balance, currency, timestamp). Manual accounts receive hand-entered snapshots.
- **Time series**: Sum all account balance snapshots over time to produce a liquid net worth series.
- **Forecast**: Prophet (augurs-prophet with wasmstan backend, MAP estimation) projects the net worth series 6–12 months forward with confidence bands. The model captures trend (are you accumulating or depleting?), seasonality (December spending dip, tax refund bump), and changepoints (salary raise, new recurring expense).

#### Budget Burndown Charts

- **Variable categories only**: Daily cumulative spend curve for the current budget month, with a predicted end-of-month landing based on recent daily rate.
- **Historical overlay**: 3 previous months rendered as ghost lines for visual comparison — instantly reveals whether current spending is faster or slower than typical.
- **Access**: Drill-down from a variable category's pace indicator on the dashboard to the burndown chart on the insights page.

#### Spending Anomaly Detection

Two layers, surfaced both on the dashboard and the insights page:

- **Structural shifts** (changepoint detection via BOCPD): Permanent level changes in category spending. "Your utilities shifted from ~€80/mo to ~€120/mo starting November." These suggest the user should update their budget.
- **One-off spikes** (outlier flagging on MSTL residuals): Single anomalous months after removing trend and seasonality. "You spent €450 on groceries in December, but that's expected seasonally" vs. "February was €45 above even the seasonal expectation."
- **Scope**: All categories with 3+ months of transaction history.
- **Dashboard integration**: Subtle badge on category rows indicating a detected anomaly, linking to detail on the insights page.

#### Seasonality-Aware Pacing

- **MSTL decomposition** per category extracts trend + seasonal pattern + residual from historical spending.
- **Seasonal expectations feed into pace calculation**: Instead of comparing spend against a flat monthly budget, the pace indicator compares against what's expected for *this specific month* based on historical patterns. €280 on groceries in December isn't "over pace" if December is always €280.
- **Trend warnings**: "Groceries trending up €24/year" — surfaces gradual lifestyle creep that's invisible in month-to-month numbers.
- **Budget adjustment suggestions**: "Your December grocery spend is consistently €70 above your €200 budget. Adjust?"

---

## Tech Stack

**Language**: Rust (all backend)

**Project structure**: Cargo workspace with domain crates:
- `core` — domain types, budget math, categorization/correlation logic
- `api` — Axum HTTP handlers, request/response types
- `db` — PostgreSQL queries and schema mapping via sqlx
- `jobs` — Apalis job definitions and handlers (sync, categorize, correlate, recompute, Amazon enrichment)
- `providers` — trait-based abstractions + implementations for bank aggregators (Enable Banking), LLM APIs, and Amazon scraping

**Web framework**: Axum (tokio-native, tower ecosystem)

**Async runtime**: Tokio

**Database**: PostgreSQL via sqlx (compile-time query checking)

**Schema migrations**: sqlx migrations (SQL files in `migrations/` folder)

**Job queue**: Apalis (persistent, retriable, observable, backed by PostgreSQL via sqlx). Atomic job types chained: sync → categorize → correlate → budget recompute. Scheduler retries failed pipelines with exponential backoff (60s–15min, up to 5 attempts).

**Time series analysis**: augurs (Rust-native). Sub-crates: augurs-prophet (wasmstan backend), augurs-mstl, augurs-ets, augurs-seasons, augurs-changepoint.

**LLM client**: reqwest + serde_json — direct REST API calls to configurable LLM provider (e.g., Gemini). No SDK dependency. Model name is a config parameter.

**Bank aggregation client**: reqwest + serde_json behind a provider trait. Current provider: Enable Banking. Swappable without affecting the rest of the system.

**Logging/observability**: tracing (structured, async-native). Logs to stderr + optional file (`/tmp/budget.log`).

**Serialization**: serde + serde_json throughout

**Frontend**: Preact + htm (lightweight React alternative), styled with Oat CSS (utility-first framework). Hash-routed SPA bundled with Bun into `frontend/dist/`, served by the Axum backend. Linting/formatting via Biome.

**Mobile**: Kotlin Multiplatform (KMP). Shared ViewModels in `commonMain` exposing `StateFlow`, consumed by Jetpack Compose on Android and SwiftUI on iOS. Networking via Ktor. Persistence via platform-specific `ConfigStore` implementations.

---

## Deployment

**Model**: Single-binary NixOS service on the home server (`tank`).

**Nix flake**: The budget repo exposes a NixOS module as a flake output. The home server's nix config (`~/nix`) imports it as a flake input:
- Input: `git+ssh://git@github.com/jeevcat/budget` (private repo access via SSH key on the server).
- Service config lives at `machines/tank/budget.nix`, following the existing per-service file pattern.

**What the flake provides**:
- `packages.${system}.default` — the compiled Rust binary
- `nixosModules.default` — a NixOS module with `services.budget.*` options (enable, port, data dir, config)

**Runtime environment**:
- Runs as a systemd service with `DynamicUser=true` (no dedicated system user needed).
- PostgreSQL database. Connection string configured via `database_url` in the config file or `DATABASE_URL` environment variable.
- Config values (LLM model, provider choice, port) are NixOS module options, wired from `secrets.toml` and module config.
- `secret_key` for API auth is read from `secrets.toml` and passed via environment variable.

**Networking**:
- Caddy reverse proxy terminates TLS and forwards to the budget service's localhost port.
- Cloudflare Tunnel exposes the service to the internet (existing pattern for all services on `tank`).
- Local network access works directly via the LAN IP + Caddy.

**Health monitoring**: Integrated with healthchecks.io via the existing `mkHealthcheckOverride` helper. The `/health` endpoint (unauthenticated) is the check target.

**Backups**: PostgreSQL database included in the existing Restic-to-Backblaze-B2 backup jobs via `pg_dump`.

---

## Out of Scope

- Savings goals / goal tracking
- Bill reminders / due date tracking
- Debt snowball/avalanche planners
- Push notifications / alerts (future work)
- Currency gain/loss tracking or revaluation
- Split transactions — one transaction, one category
- Shared / multi-user access — single user only (multiple accounts supported)
