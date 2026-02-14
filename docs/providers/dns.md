# DNS Provider Interface

This document describes the `Dns` trait — its design, the reasoning behind it, and what you need to know when implementing a new DNS provider.

## The Trait

```rust
pub trait Dns {
    type Config: serde::de::DeserializeOwned;
    type Error: std::error::Error + Send + Sync + 'static;
    type Key;

    fn new(config: Self::Config) -> Result<Self, Self::Error>;
    async fn add_record(&self, record: Record) -> Result<Self::Key, Self::Error>;
    async fn delete_record(&self, key: Self::Key) -> Result<(), Self::Error>;
    async fn list_records(&self) -> Result<Vec<(Self::Key, Record)>, Self::Error>;
    async fn update_record(&self, key: Self::Key, record: Record) -> Result<(), Self::Error>;
}
```

## Associated Types

### `Config`

Each provider defines its own configuration struct. Must implement `DeserializeOwned` so it can be loaded from TOML.

**Examples:**
- Porkbun needs `domain`, `api_key`, `secret_key`
- Hickory needs `file_path`, `tcp_addr`, `udp_addr`

### `Error`

Provider-specific error type. Must be `Send + Sync + 'static` for async compatibility. Wrap your upstream errors here.

### `Key`

This is the interesting one. **Different providers identify records differently:**

| Provider | Key Type | Why |
|----------|----------|-----|
| Porkbun | `String` | API returns string record IDs |
| Hickory | `u64` | Auto-incrementing numeric IDs in our BTreeMap |
| Dummy | `usize` | Vec index |

The `Key` is opaque to the server — it just stores the key (as a string in the database) and passes it back when updating or deleting. The provider is responsible for understanding what the key means.

## The Record Type

```rust
pub struct Record {
    pub name: String,           // Full subdomain, e.g. "alice.is.fckn.gay"
    pub record_type: RecordType,
    pub content: String,        // The record data (IP, domain, text, etc.)
    pub ttl_seconds: u32,       // Minimum 300 seconds
}
```

**Note:** MX priority is included in `content` (e.g., `"10 mail.example.com"`) rather than as a separate field.

### Supported Record Types

```rust
pub enum RecordType {
    A, AAAA, CNAME, ALIAS, MX, NS, TXT, SRV, TLSA, CAA, HTTPS, SVCB
}
```

Note: `ALIAS` is non-standard and may not be supported by all providers. Handle gracefully.

## Design Decisions

### Why `Key` is an Associated Type

We considered a few alternatives:

1. **Always use `String`**: Works, but forces providers to serialize/deserialize their native IDs
2. **Use a trait object**: Adds complexity and heap allocation
3. **Associated type**: Each provider uses its natural ID type, we store it as a string in the database

We went with (3). The database stores `provider_key: String`, and it's up to the provider to parse it back when needed. This keeps the trait clean and providers simple.

### Why Records Use Full Names

The `name` field contains the full subdomain (e.g., `alice.is.fckn.gay`) rather than just the user's part (`alice`). This is because:

1. Providers like Porkbun want the subdomain relative to the base domain
2. Providers like Hickory want the full FQDN
3. It's easier to strip a suffix than to know what to add

Each implementor is responsible for handling the name format their upstream expects.

### Why No `get_record` Method

We only have `list_records`, not `get_record(key)`. This is because:

1. The database is our source of truth, not the DNS provider
2. Listing is needed for sync/reconciliation anyway
3. Most operations already have the record data from the database

If you need a single record, query the database — that's where ownership info lives too.

## Implementing a New Provider

1. Create a new crate at `dns/implementors/your-provider/`
2. Define your `Config` struct with serde
3. Define your `Error` type (wrap upstream errors)
4. Implement the trait, choosing an appropriate `Key` type
5. Add a feature flag in `dns/Cargo.toml`
6. Re-export from `dns/src/lib.rs` behind the feature

### Checklist

- [ ] `new()` validates config and creates client/connection
- [ ] `add_record()` returns a key that can be used for update/delete
- [ ] `delete_record()` handles "not found" gracefully (idempotent is nice)
- [ ] `list_records()` returns all records (for sync purposes)
- [ ] `update_record()` updates in place (some APIs require delete + add)
- [ ] Handle the `name` field format your provider expects
- [ ] Handle record types your provider doesn't support (return error, not panic)

## Current Implementors

### Dummy (`dns/implementors/dummy/`)

In-memory storage for testing. Records live in a `Vec`, keys are indices. No persistence.

### Hickory (`dns/implementors/hickory/`)

Self-hosted DNS server using the Hickory DNS library. Records stored in a TOML file, served over TCP/UDP. The server runs as a background task spawned in `new()`.

**Key quirks:**
- Panics on unsupported record types (should be fixed)
- File persistence is fragile (can corrupt on crash mid-write)
- Runs its own DNS server on configured ports

### Porkbun (`dns/implementors/porkbun/`)

Cloud DNS via Porkbun's API. Uses the `porkbun-api` crate.

**Key quirks:**
- `update_record` not implemented yet (TODO)
- Validates subdomain belongs to configured domain
- Rejects non-ASCII subdomains

## Related Documents

The DNS provider is just one piece of the puzzle. For how records flow through the system:

- **[../flows/api.md](../flows/api.md)** — DNS record CRUD, transaction flows, divergence handling
