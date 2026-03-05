# Parse Don't Validate — Remaining Opportunities

## Newtypes

### ISO 20022 Domain/Sub-family Codes
- `Transaction.bank_transaction_code_code` — e.g. `PMNT`
- `Transaction.bank_transaction_code_sub_code` — e.g. `ICDT-STDO`
- Note: large open set, may be better as validated newtypes than enums

## Integer Bounds

### Pagination
- ~`ListQuery.limit` — should reject values outside 1–200 at deserialization, not clamp in handler~
- ~`ListQuery.offset` — should reject negative values at deserialization~

### Priority
- `Rule.priority` / `CreateRule.priority` — unbounded `i32`, consider newtype with 0–1000 range

### ValidDays
- `AuthorizeRequest.valid_days` — `u32` with no upper bound, could request absurd durations

## Config Validation

### Config struct (`core/src/lib.rs`)
- `database_url: String` → could validate as URL at load time
- `bank_provider: String` → enum of supported providers
- `secret_key: String` → minimum length enforcement
- `host: Option<String>` → URL newtype

## Related Option Fields → Enums

### Categorization state
`category_id` + `category_method` + `suggested_category` on Transaction could become:
```rust
enum Categorization {
    Uncategorized,
    Manual(CategoryId),
    Rule(CategoryId),
    Llm { category_id: CategoryId, suggested: String },
}
```

### NicknameUpdate
`UpdateNickname.nickname: Option<String>` overloads `None` for "clear":
```rust
enum NicknameUpdate { Set(String), Clear }
```

### Account connection state
`Account.connection_id: Option<ConnectionId>` conflates "manual" vs "connected":
```rust
enum AccountOrigin { Manual, Connected(ConnectionId) }
```
