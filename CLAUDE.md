# Budget

## Setup

Activate the pre-commit hook:
```bash
ln -sf ../../.github/hooks/pre-commit .git/hooks/pre-commit
```

## Production API

Server runs on port 3001. Use `scripts/api` for all local API calls — it injects the auth token so secrets never appear in command output.

```bash
scripts/api /budgets/status                          # GET (default)
scripts/api -X POST /rules/apply                     # POST
scripts/api -X POST -d '{"key":"val"}' /some/endpoint  # POST with body
```

### Test mode

`scripts/api --test` spins up a temp database and ephemeral dev server, runs the request, then tears everything down. Useful for testing endpoints against a clean database without touching production data.

```bash
scripts/api --test /accounts                         # GET against empty database
scripts/api --test /docs                             # fetch OpenAPI/Scalar docs page
scripts/api --test -X POST -d '{"name":"Checking","account_type":"checking","currency":"EUR"}' /accounts
```

## API Documentation

- Browse interactively: `http://localhost:3001/api/docs` (Scalar UI)
- Fetch the spec via script: `scripts/api /docs` (production) or `scripts/api --test /docs` (temp server)
- All handlers are annotated with `#[utoipa::path]`; schemas are auto-generated from types via `#[derive(ToSchema)]`
- Cross-crate schemas gated behind the `openapi` cargo feature flag

## Config

Path is determined by `confy` via the `directories` crate. Run `cargo run -- config` to print the resolved path. On this machine: `~/.config/budget/default-config.toml`.

## Testing

- Live backend tests (Enable Banking sandbox, Gemini API) use `#[ignore = "hits live ..."]`
- Run them explicitly with: `cargo test -p budget-providers -- --ignored`
- All other tests use PostgreSQL (`#[sqlx::test]` creates temp databases) + mock providers
- Requires `DATABASE_URL` env var pointing to a PG instance (e.g. `postgresql://budget@localhost:5432/budget`)
- The `budget` PG user needs `CREATEDB` permission for `sqlx::test` to work

## Logging

- Logs go to stderr and optionally to a file (when `log_path` is set in config)
- Production log file: `/tmp/budget.log`
- Read logs: `tail -f /tmp/budget.log` (via Bash tool)
- Change verbosity: set `RUST_LOG` env var (default: `budget=debug,tower_http=debug,info`)
- `cargo run -- config` prints both the config path and log path

## Frontend

- **Bun** is the package manager and bundler — `bun install` for deps, `bun run build` to bundle into `frontend/dist/`
- Source files live in `frontend/` (app.js, style.css, index.html); the server serves `frontend/dist/`
- **Strongly prefer [Oat CSS](https://oat.ink/) classes and components over custom CSS** — always check `frontend/oat-reference.md` and Oat utility classes (`hstack`, `vstack`, `text-light`, `gap-*`, `badge`, `card`, `chip`, etc.) before writing anything in `style.css`
- Custom CSS in `style.css` is a last resort — only for things Oat genuinely cannot do (custom visualizations, pseudo-elements, responsive grid breakpoints, sticky table headers)
- When reviewing or modifying frontend code, actively look for opportunities to replace existing custom CSS with Oat equivalents
- **Use Biome for linting/formatting** — run `bun run check`, fix with `bun run fix`. Never use node/python hacks for syntax checks.
- **Frontend tests** — `bun test frontend/` (bun's built-in test runner). Pure helpers live in `frontend/helpers.js`, tested by `frontend/helpers.test.js`. Pre-commit hook runs tests automatically.
- **E2E tests** — `bun run e2e` (Playwright). Creates an isolated temp database, starts the server via `webServer`, authenticates via `storageState`. Requires PostgreSQL running and `chromium` on `PATH` (provided by the Nix dev shell). Stale databases from crashed runs are cleaned up automatically on next run.

## Mobile

### Architecture

- **Kotlin Multiplatform (KMP)** — share as much non-presentation code as possible between Android and iOS
- **Shared ViewModel** — use AndroidX `lifecycle-viewmodel` (KMP since 2.8+) in `commonMain`; ViewModels expose `StateFlow` consumed by platform UI
- **Persistence** — `ConfigStore` interface in `commonMain` with platform implementations (`AndroidConfigStore` via SharedPreferences); simple key-value config doesn't need DataStore
- **Networking** — **Ktor** HTTP client (KMP-compatible, shared across platforms)
- **Navigation** — platform-native (no shared navigation library); shared ViewModels handle logic, platform screens handle routing
- **iOS Flow consumption** — SKIE transforms Kotlin `StateFlow` into Swift `AsyncSequence` for seamless SwiftUI integration

### Conventions

- Presentation layer is platform-specific: Jetpack Compose on Android, SwiftUI on iOS
- Screens are thin render layers: observe `StateFlow` from shared ViewModels, forward user events back
- Business logic (validation, API calls, config persistence) lives in shared ViewModels, not in Compose/SwiftUI code
- Use `ConnectionTester` (fun interface) to abstract API calls in ViewModels for unit testability
- ViewModel tests go in `shared/src/commonTest` using `kotlinx-coroutines-test` with fake implementations

## Coding Standards

- **Never suppress clippy lints** without explicit human approval
- **No `unwrap()` or `expect()`** in production code — use `Result<T, E>`
- **No `clone()` without justification**
- Parse, don't validate — use newtypes to make invalid states unrepresentable
- Prefer `&str` over `String` when ownership isn't needed
- **Never add, remove, or update dependencies** without explicit human approval
- When completing a TODO item, delete the line from `TODO.md` — don't mark it `[x]`
