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
- All other tests use in-memory SQLite + mock providers and run fast

## Logging

- Logs go to stderr and optionally to a file (when `log_path` is set in config)
- Read logs: `tail -f ~/.config/budget/budget.log` (via Bash tool)
- Change verbosity: set `RUST_LOG` env var (default: `budget=debug,tower_http=debug,info`)
- `cargo run -- config` prints both the config path and log path

## Coding Standards

- **Never suppress clippy lints** without explicit human approval
- **No `unwrap()` or `expect()`** in production code — use `Result<T, E>`
- **No `clone()` without justification**
- Parse, don't validate — use newtypes to make invalid states unrepresentable
- Prefer `&str` over `String` when ownership isn't needed
- **Never add, remove, or update dependencies** without explicit human approval
