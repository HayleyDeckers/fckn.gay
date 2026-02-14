# Database Provider Interface

This document describes the `UserDatabase` trait, the schema design, and our decisions around data modeling.

## The Trait

```rust
pub trait UserDatabase {
    type Config: serde::de::DeserializeOwned;
    type Error: std::error::Error + Send + Sync + 'static;

    fn new(config: Self::Config) -> Result<Self, Self::Error>;

    // User management
    async fn is_valid(&self, email: &str, password: &str) -> bool;
    async fn validate_and_get_user_id(&self, email: &str, password: &str) -> Option<Uuid>;
    async fn is_available(&self, email: &str) -> bool;
    async fn add_user(&self, email: &str, password: &str) -> Result<Uuid, Self::Error>;
    async fn delete_user(&self, email: &str) -> Result<(), Self::Error>;
    async fn activate_user(&self, uuid: Uuid) -> Result<(), Self::Error>;

    // Domain management
    async fn claim_domain(&self, user_id: Uuid, domain_name: &str) -> Result<(), Self::Error>;
    async fn get_user_domains(&self, user_id: Uuid) -> Result<Vec<String>, Self::Error>;
    async fn domain_owner(&self, domain_name: &str) -> Result<Option<Uuid>, Self::Error>;

    // DNS record management
    async fn add_dns_record(&self, domain_name: &str, record: DnsRecord, provider_key: String) -> Result<DnsRecordId, Self::Error>;
    async fn get_domain_dns_records(&self, domain_name: &str) -> Result<Vec<DatabaseDnsRecord>, Self::Error>;
    async fn update_dns_record(&self, record_id: DnsRecordId, record: DnsRecord) -> Result<(), Self::Error>;
    async fn delete_dns_record(&self, record_id: DnsRecordId) -> Result<(), Self::Error>;
    async fn get_dns_record_provider_key(&self, record_id: DnsRecordId) -> Result<String, Self::Error>;

    // Session management
    async fn create_session(&self, user_id: Uuid, token: &str, expires_at: DateTime) -> Result<(), Self::Error>;
    async fn get_session(&self, token: &str) -> Result<Option<Session>, Self::Error>;
    async fn delete_session(&self, token: &str) -> Result<(), Self::Error>;
    async fn delete_expired_sessions(&self) -> Result<u64, Self::Error>;
}
```

Note: This is the *target* trait design. Current implementation may differ — see "Current State vs Target" below.

## Schema

### users

Registered accounts. Email is the identifier (no separate username).

| Column | Type | Notes |
|--------|------|-------|
| `id` | uuid | Primary key |
| `email` | string | Unique, used for login |
| `password_hash` | string | Argon2 hash |
| `state` | enum | pending, active, inactive, banned |
| `created_at` | timestamp | |
| `last_login` | timestamp? | Nullable, updated on login |

**User states:**
- `pending` — signed up but email not confirmed
- `active` — email confirmed, can use the service
- `inactive` — soft-deleted or voluntarily deactivated
- `banned` — suspended by admin

### domains

Claimed subdomains. One user can own multiple domains.

| Column | Type | Notes |
|--------|------|-------|
| `domain_name` | string | Primary key, lowercase (e.g., `alice.is.fckn.gay`) |
| `user_id` | uuid | FK → users |
| `claimed_at` | timestamp | |

Using `domain_name` as PK because:
- It's already unique by definition
- It's what we query by most often
- Makes the dns_records FK human-readable

DNS names are case-insensitive (RFC 1035), so we store lowercase.

### dns_records

DNS records owned by users, tracked for sync with upstream providers.

| Column | Type | Notes |
|--------|------|-------|
| `id` | uuid | Primary key |
| `domain_name` | string | FK → domains |
| `provider_key` | string | Upstream provider's record ID |
| `record_type` | enum | A, AAAA, CNAME, MX, TXT, etc. |
| `record_name` | string | Full DNS name, lowercase |
| `content` | string | Record data (IP, domain, text, etc.) |
| `ttl_seconds` | u32 | Minimum 300 |

**Notes:**
- `domain_name` FK ensures ownership — if user owns the domain, they own records under it
- `record_name` is the full DNS name (e.g., `www.alice.is.fckn.gay`)
- `provider_key` is opaque to us, just passed back to the DNS provider for updates/deletes
- MX priority goes in `content` (e.g., `"10 mail.example.com"`)

### sessions

Login sessions. Mirrored from in-memory cache for persistence.

| Column | Type | Notes |
|--------|------|-------|
| `token` | string | Primary key, 128-bit random hex |
| `user_id` | uuid | FK → users |
| `expires_at` | timestamp | |
| `created_at` | timestamp | |
| `permissions` | string? | Future: scopes for API tokens |

**Design:**
- In-memory cache is primary (fast reads)
- Database is write-through (survives restarts)
- On startup, load unexpired sessions from DB into cache

## Design Decisions

### Email as Identifier (No Username)

We dropped the separate username field:
- Email is already unique and verified
- One less thing for users to remember
- Subdomain claiming is now explicit (via domains table) rather than implicit from username

### Domains as Separate Table

Considered storing domains as a JSON list on users, but:
- Need fast "who owns this domain?" lookups for validation
- List would require full table scan
- Separate table = index on domain_name, O(1) lookup

### DNS Records Linked to Domains, Not Users

Records FK to `domains.domain_name` rather than `users.id`:
- Ownership check is automatic via the FK
- If user owns the domain, they own all records under it
- No need for separate ownership validation queries

### Sessions: Memory + Database

Pure in-memory is fast but loses sessions on restart. Pure database adds latency to every request. Hybrid approach:
- Memory for reads (fast path)
- Write-through to database (persistence)
- Startup hydration from database

## Current State vs Target

The current implementation differs from this design:

| Feature | Current | Target |
|---------|---------|--------|
| User identifier | username | email |
| Domain ownership | implicit from username | explicit domains table |
| DNS record ownership | user_id FK | domain_name FK |
| Sessions | in-memory only | memory + database |

Migration path: incremental updates to the Diesel implementor, then update the trait.

## Current Implementors

### Dummy (`user-database/implementors/dummy/`)

In-memory HashMap storage. Useful for testing. No persistence.

### CSV (`user-database/implementors/csv/`)

Flat file storage. Simple but limited — no proper transactions, no concurrent access safety.

### Diesel (`user-database/implementors/diesel/`)

SQLite via Diesel ORM. The "real" implementation for production use.

**Current schema** (will evolve toward target):
- users table with username field
- dns_records with user_id FK

## Future Work

### Sync Status Tracking

For the DNS checker service, add to dns_records:
- `last_synced_at: timestamp?` — when we last verified upstream matches
- `error_message: string?` — null means synced, non-null means error

The checker service would:
1. Periodically compare DB records to upstream provider
2. Set `error_message` if mismatch found
3. Dashboard/alerts for records with errors
4. Manual or automatic remediation

### Email Queue

If we add email queuing (see [email.md](email.md)), it could live in this database:

```
email_queue
  - id: uuid
  - to: string
  - subject: string
  - body: string
  - status: enum (pending/sending/sent/failed/bounced)
  - retry_count: u32
  - next_retry_at: timestamp?
  - sent_at: timestamp?
  - error_message: string?
```

Or use a separate service (SendGrid, Postmark, etc.) which handles queuing for us.

### API Tokens

The `sessions.permissions` field is a placeholder for API token scopes:
- `null` — full access (regular login session)
- `"dns:read"` — read-only DNS access
- `"dns:read,dns:write"` — DNS management

Would need UI for users to create/revoke tokens.

### Horizontal Scaling

SQLite is single-node. For horizontal scaling:
- PostgreSQL for the database
- Redis or external session store for sessions
- Or go stateless with JWTs (but then can't revoke sessions easily)
