# Parse Don't Validate — Remaining Opportunities

## Config Validation

### Config struct (`core/src/lib.rs`)
- `database_url: String` → could validate as URL at load time
- `bank_provider: String` → enum of supported providers
- `secret_key: String` → minimum length enforcement
- `host: Option<String>` → URL newtype

## Related Option Fields → Enums

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
