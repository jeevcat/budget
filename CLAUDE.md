# Budget

## Setup

Activate the pre-commit hook:
```bash
ln -sf ../../.github/hooks/pre-commit .git/hooks/pre-commit
```

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

- **Strongly prefer [Oat CSS](https://oat.ink/) classes and components over custom CSS** — always check `frontend/oat-reference.md` and Oat utility classes (`hstack`, `vstack`, `text-light`, `gap-*`, `badge`, `card`, `chip`, etc.) before writing anything in `style.css`
- Custom CSS in `style.css` is a last resort — only for things Oat genuinely cannot do (custom visualizations, pseudo-elements, responsive grid breakpoints, sticky table headers)
- When reviewing or modifying frontend code, actively look for opportunities to replace existing custom CSS with Oat equivalents

## Mobile

- **Kotlin Multiplatform (KMP)** — share as much non-presentation code as possible between Android and iOS
- **Ktor** for HTTP networking (KMP-compatible, shared across platforms)
- Presentation layer is platform-specific: Jetpack Compose on Android, SwiftUI on iOS

## Coding Standards

- **Never suppress clippy lints** without explicit human approval
- **No `unwrap()` or `expect()`** in production code — use `Result<T, E>`
- **No `clone()` without justification**
- Parse, don't validate — use newtypes to make invalid states unrepresentable
- Prefer `&str` over `String` when ownership isn't needed
- **Never add, remove, or update dependencies** without explicit human approval
