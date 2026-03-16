# TODO

## Bugs


## From Spec — Remaining Work

### Edge Case Coverage
- [ ] **Late/missing salary UX**: Verify the previous budget month stays open indefinitely and surface a clear signal when expected salaries haven't arrived

### Frontend
- [ ] **Error boundaries + retry**: Errors replace entire page content with no retry button. Add a retry mechanism

## Insights & Analytics

### Features
- [ ] **Spending anomaly detection**: Changepoint detection (BOCPD) for structural shifts ("groceries shifted +35% in October") + outlier flagging on MSTL residuals for one-off spikes. Surface on dashboard as subtle badge on category rows linking to detail on insights page. Run on all categories with 3+ months of history

## Blocked Upstream

- [ ] **Gradle in Claude Code sandbox**: `dl.google.com` / `maven.google.com` are blocked by the sandbox egress proxy (403 `host_not_allowed`), so Gradle can't resolve AGP or Google Maven deps. The pre-commit hook gracefully skips Kotlin compilation when this happens. Once [anthropics/claude-code#16222](https://github.com/anthropics/claude-code/issues/16222) is fixed, remove the skip logic from `.github/hooks/pre-commit` and verify `./gradlew compileDebugKotlin` works in sandbox sessions
