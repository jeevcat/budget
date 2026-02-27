# TODO

## From Spec — Remaining Work

### Backfill Logic
- [ ] **Inter-month gap transaction assignment**: Transactions posted between calendar month start and salary arrival should be assigned to the previous (still-open) budget month
- [ ] **First-time backfill verification**: Verify retroactive budget month detection covers full imported history end-to-end

### Edge Case Coverage
- [ ] **Rule conflict resolution**: Test priority ordering when multiple rules match the same transaction (merchant regex vs. amount range vs. description pattern)
- [ ] **Late/missing salary UX**: Verify the previous budget month stays open indefinitely and surface a clear signal when expected salaries haven't arrived

### Frontend Polish
- [ ] **Budget month transaction view**: No way to view transactions scoped to a specific budget month
- [ ] **Rollover visualization**: Dashboard doesn't show rollover surplus/deficit carried from prior months

## From Spec — Deviations

- [x] **Auth: HttpOnly cookie**: Token is now stored in an HttpOnly cookie. Login via `POST /api/login`, logout via `POST /api/logout`. Bearer header still accepted for API clients
- [x] **Database: SQLite → PostgreSQL**: Config default updated to PostgreSQL. SPEC updated accordingly

## Architecture Review Findings

### High Priority
- [x] **Pagination on transaction list**: `GET /api/transactions` returns all transactions. Add cursor/offset pagination to avoid multi-MB responses as history grows
- [x] **Budget status endpoint optimization**: `/api/budgets/status` loads all transactions into memory then filters. Query only current period's transactions in SQL

### Medium Priority
- [ ] **Stop leaking DB errors to clients**: `From<sqlx::Error>` in `routes/mod.rs` sends raw `e.to_string()` as response body, exposing schema details. Return generic "Database error" message instead
- [ ] **Make salary category configurable**: `budget.rs` hardcodes `c.name == "Salary"` for budget month detection. Add a config field so non-English or differently-named categories work
- [x] **TEXT UUIDs → native UUID type**: Migrated to native PostgreSQL UUID columns with sqlx `uuid` feature. ID newtypes implement `sqlx::Type/Encode/Decode` directly
- [ ] **Deduplicate budget logic**: Frontend re-implements category subtree traversal, transaction filtering, and month boundary computation. Consider having the frontend rely on the `/api/budgets/status` response instead

### Low Priority
- [ ] **Constant-time token comparison**: `auth.rs` uses `==` for bearer token check. Use `subtle::ConstantTimeEq` for timing-attack resistance
- [ ] **Error boundaries + retry in frontend**: Errors replace entire page content with no retry button. Add a retry mechanism
- [ ] **Duplicate MatchField enum**: `MatchField` defined in both `core/models/enums.rs` (7 variants) and `providers/llm.rs` (6 variants) with manual mapping in `routes/transactions.rs`
- [ ] **Magic timeouts**: Fixed 5s polling on Jobs page, hardcoded 1500ms delay after rule creation. Poll adaptively or use SSE
