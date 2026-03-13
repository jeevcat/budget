# TODO

## Bugs


## From Spec — Remaining Work

### Backfill Logic
- [ ] **Inter-month gap transaction assignment**: Transactions posted between calendar month start and salary arrival should be assigned to the previous (still-open) budget month
- [ ] **First-time backfill verification**: Verify retroactive budget month detection covers full imported history end-to-end

### Edge Case Coverage
- [ ] **Late/missing salary UX**: Verify the previous budget month stays open indefinitely and surface a clear signal when expected salaries haven't arrived

### Frontend
- [ ] **Budget month transaction view**: No way to view transactions scoped to a specific budget month
- [ ] **Error boundaries + retry**: Errors replace entire page content with no retry button. Add a retry mechanism
- [ ] **Magic timeouts**: Fixed 5s polling on Jobs page, hardcoded 1500ms delay after rule creation. Poll adaptively or use SSE

## Parse Don't Validate

- [ ] **NicknameUpdate**: `UpdateNickname.nickname: Option<String>` overloads `None` for "clear" → `enum NicknameUpdate { Set(String), Clear }`
- [ ] **Account connection state**: `Account.connection_id: Option<ConnectionId>` conflates "manual" vs "connected" → `enum AccountOrigin { Manual, Connected(ConnectionId) }`

## Blocked Upstream

- [ ] **Gradle in Claude Code sandbox**: `dl.google.com` / `maven.google.com` are blocked by the sandbox egress proxy (403 `host_not_allowed`), so Gradle can't resolve AGP or Google Maven deps. The pre-commit hook gracefully skips Kotlin compilation when this happens. Once [anthropics/claude-code#16222](https://github.com/anthropics/claude-code/issues/16222) is fixed, remove the skip logic from `.github/hooks/pre-commit` and verify `./gradlew compileDebugKotlin` works in sandbox sessions
