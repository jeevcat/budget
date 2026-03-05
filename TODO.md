# TODO

## From Spec — Remaining Work

### Backfill Logic
- [ ] **Inter-month gap transaction assignment**: Transactions posted between calendar month start and salary arrival should be assigned to the previous (still-open) budget month
- [ ] **First-time backfill verification**: Verify retroactive budget month detection covers full imported history end-to-end

### Edge Case Coverage
- [ ] **Late/missing salary UX**: Verify the previous budget month stays open indefinitely and surface a clear signal when expected salaries haven't arrived

### Frontend Polish
- [ ] **Budget month transaction view**: No way to view transactions scoped to a specific budget month

## Architecture Review Findings

### Medium Priority
- [ ] **Stop leaking DB errors to clients**: `From<sqlx::Error>` in `routes/mod.rs` sends raw `e.to_string()` as response body, exposing schema details. Return generic "Database error" message instead

### Blocked Upstream
- [ ] **Gradle in Claude Code sandbox**: `dl.google.com` / `maven.google.com` are blocked by the sandbox egress proxy (403 `host_not_allowed`), so Gradle can't resolve AGP or Google Maven deps. The pre-commit hook gracefully skips Kotlin compilation when this happens. Once [anthropics/claude-code#16222](https://github.com/anthropics/claude-code/issues/16222) is fixed, remove the skip logic from `.github/hooks/pre-commit` and verify `./gradlew compileDebugKotlin` works in sandbox sessions

### Low Priority
- [ ] **Constant-time token comparison**: `auth.rs` uses `==` for bearer token check. Use `subtle::ConstantTimeEq` for timing-attack resistance
- [ ] **Error boundaries + retry in frontend**: Errors replace entire page content with no retry button. Add a retry mechanism
- [ ] **Duplicate MatchField enum**: `MatchField` defined in both `core/models/enums.rs` (7 variants) and `providers/llm.rs` (6 variants) with manual mapping in `routes/transactions.rs`
- [ ] **Magic timeouts**: Fixed 5s polling on Jobs page, hardcoded 1500ms delay after rule creation. Poll adaptively or use SSE
