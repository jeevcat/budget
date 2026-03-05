# Parse Don't Validate — Remaining Opportunities

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
