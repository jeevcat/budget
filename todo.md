# Parse Don't Validate — Remaining Opportunities

## Newtypes

### CurrencyCode
3-uppercase-ASCII-char ISO 4217 newtype. Used in ~5 places:
- `Config.budget_currency` (`core/src/lib.rs`)
- `Account.currency` (`core/src/models/domain.rs`)
- `Transaction.original_currency`
- `Transaction.exchange_rate_unit_currency`
- `Transaction.balance_after_transaction_currency`

### IBAN / BIC
- `Transaction.counterparty_iban` — validate format + checksum
- `Transaction.counterparty_bic` — 8 or 11 character format

### MerchantCategoryCode
- `Transaction.merchant_category_code` — 4-digit ISO 18245 code

## Enums for Structured Banking Codes

### ExchangeRateType
- `Transaction.exchange_rate_type` — closed set: `AGRD`, `SALE`, `SPOT`

### ReferenceNumberSchema
- `Transaction.reference_number_schema` — closed set: `BERF`, `FIRF`, `INTL`, `NORF`, `SDDM`, `SEBG`
- Note: these come from Enable Banking, so the set may expand — consider `#[serde(other)]`

### ISO 20022 Domain/Sub-family Codes
- `Transaction.bank_transaction_code_code` — e.g. `PMNT`
- `Transaction.bank_transaction_code_sub_code` — e.g. `ICDT-STDO`
- Note: large open set, may be better as validated newtypes than enums

## Integer Bounds

### Pagination
- `ListQuery.limit` — should reject values outside 1–200 at deserialization, not clamp in handler
- `ListQuery.offset` — should reject negative values at deserialization

### Priority
- `Rule.priority` / `CreateRule.priority` — unbounded `i32`, consider newtype with 0–1000 range

### ValidDays
- `AuthorizeRequest.valid_days` — `u32` with no upper bound, could request absurd durations

### SalaryTransactionsDetected
- `BudgetMonth.salary_transactions_detected` — `i32` for a count, should be `u32`

### ExpectedSalaryCount
- `Config.expected_salary_count` — `u32`, zero makes no sense, should be `NonZeroU32`

## Job Queue Typed IDs

### CategorizeTransactionJob / CorrelateTransactionJob
- `transaction_id: String` — should be `TransactionId` (parsed at enqueue time, not dequeue)

### SyncJob
- `account_id: String` — should be `AccountId`

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
