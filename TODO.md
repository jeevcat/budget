# TODO

## Bugs


## From Spec — Remaining Work

### Edge Case Coverage
- [ ] **Late/missing salary UX**: Verify the previous budget month stays open indefinitely and surface a clear signal when expected salaries haven't arrived

### Frontend
- [ ] **Error boundaries + retry**: Errors replace entire page content with no retry button. Add a retry mechanism

## Insights & Analytics

### Features
- [ ] **Budget burndown charts**: Daily cumulative spend curve for variable-mode categories with predicted end-of-month landing. Overlay 3 previous months as ghost lines for comparison. Lives on insights page as drill-down from dashboard categories
- [ ] **Spending anomaly detection**: Changepoint detection (BOCPD) for structural shifts ("groceries shifted +35% in October") + outlier flagging on MSTL residuals for one-off spikes. Surface on dashboard as subtle badge on category rows linking to detail on insights page. Run on all categories with 3+ months of history
- [ ] **Seasonality-aware pacing**: MSTL decomposition per category feeds seasonal expectations into pace calculation — "over budget" accounts for the fact that December is always expensive. Powers trend warnings ("groceries trending up €24/year") and budget adjustment suggestions ("your December grocery spend is consistently €70 above budget")

### Integration
- [ ] **Expand augurs features**: Add `mstl`, `ets`, `seasons`, `changepoint` features to augurs dependency when implementing anomaly detection and seasonality-aware pacing
- [ ] **Insights page**: Add burndown drill-downs to the existing insights page. Keep lightweight — most intelligence surfaces on the existing dashboard via enhanced pace indicators and anomaly badges

## Blocked Upstream

- [ ] **Gradle in Claude Code sandbox**: `dl.google.com` / `maven.google.com` are blocked by the sandbox egress proxy (403 `host_not_allowed`), so Gradle can't resolve AGP or Google Maven deps. The pre-commit hook gracefully skips Kotlin compilation when this happens. Once [anthropics/claude-code#16222](https://github.com/anthropics/claude-code/issues/16222) is fixed, remove the skip logic from `.github/hooks/pre-commit` and verify `./gradlew compileDebugKotlin` works in sandbox sessions
