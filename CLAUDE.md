# Budget

## Setup

Activate the pre-commit hook:
```bash
ln -sf ../../.github/hooks/pre-commit .git/hooks/pre-commit
```

## Coding Standards

- **Never suppress clippy lints** without explicit human approval
- **No `unwrap()` or `expect()`** in production code — use `Result<T, E>`
- **No `clone()` without justification**
- Parse, don't validate — use newtypes to make invalid states unrepresentable
- Prefer `&str` over `String` when ownership isn't needed
- **Never add, remove, or update dependencies** without explicit human approval
