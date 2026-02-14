# Implementation Guide

This document covers the practical details of working on the codebase — workspace structure, how crates are organized, and how to configure the server.

For the high-level architecture and design goals, see [overview.md](overview.md).

## Workspace Structure

```
this-workspace-is-fckn-gay/
├── server/           # Main Axum web server
├── dns/              # DNS provider interface + implementors
├── email/            # Email provider interface + implementors
├── user-database/    # User storage interface + implementors
├── validation/       # Shared validation (compiles to WASM too)
└── secret/           # Flexible secret handling (env, file, inline)
```

## The Server

The `server` crate is the only runnable binary. It:

- Boots an Axum web server
- Loads configuration from TOML
- Instantiates the configured providers
- Wires up routes for auth and API
- Applies rate limiting and middleware

The server doesn't know or care which specific implementations are being used — it just talks to the traits.

## Interface Crate Anatomy

Each provider type (DNS, email, user-database) follows this structure:

```
dns/
├── interface/        # The trait definition (fckn-gay-dns-interface)
├── implementors/
│   ├── dummy/        # In-memory test implementation
│   ├── hickory/      # Self-hosted DNS server
│   └── porkbun/      # Porkbun API client
└── src/lib.rs        # Re-exports interface + feature-gated implementors
```

**Terminology:**
- **Interface** = the trait definition (what methods exist)
- **Implementor** = a concrete implementation (Porkbun, Hickory, etc.)
- **Provider** = we use this loosely to mean either

The top-level crate (`fckn-gay-dns`) re-exports the trait and whichever implementors are enabled via feature flags. The server only depends on this top-level crate.

## Shared Crates

### `validation`

Email/password validation logic that compiles to both native Rust and WASM. The same rules run server-side and client-side (in the frontend).

### `secret`

A helper for loading secrets from different sources. Used in provider configs:

```toml
# From environment variable
api_key = { env = "PORKBUN_API_KEY" }

# From file
api_key = { file = "/run/secrets/porkbun_key" }

# Inline (not recommended for secrets)
api_key = { value = "sk_live_..." }
```

## Configuration

Providers are selected at runtime via `config.toml`:

```toml
dns.provider = "porkbun"           # or "hickory", "dummy"
email.provider = "lettre"          # or "dummy"
user_database.provider = "diesel"  # or "csv", "dummy"

[dns.porkbun]
api_key = { env = "PORKBUN_API_KEY" }
secret_key = { env = "PORKBUN_SECRET_KEY" }
domain = "is.fckn.gay"

[user_database.diesel]
database_url = "sqlite.db"
```

### Environment Configurations

| Environment | DNS | Email | Database | Notes |
|-------------|-----|-------|----------|-------|
| Local dev | `dummy` | `dummy` | `dummy` | No external deps |
| Integration test | `dummy` | `dummy` | `csv` | Persistent but simple |
| Production | `porkbun` | `lettre` | `diesel` | The real deal |

All configurations work without recompiling (assuming the relevant feature flags are enabled in the build).

## Adding a New Provider

1. Create a new crate at `<interface>/implementors/<your-provider>/`
2. Define your `Config` struct with serde derives
3. Define your `Error` type (wrap upstream errors)
4. Implement the trait
5. Add a feature flag in `<interface>/Cargo.toml`
6. Re-export from `<interface>/src/lib.rs` behind the feature

See the existing implementors for examples. The `dummy` implementor is the simplest reference.

## Feature Flags

Each implementor is behind a feature flag:

```toml
# dns/Cargo.toml
[features]
default = []
dummy = ["fckn-gay-dns-dummy"]
hickory = ["fckn-gay-dns-hickory"]
porkbun = ["fckn-gay-dns-porkbun"]
```

Build with the providers you need:

```bash
cargo build --features "dns/porkbun,email/lettre,user-database/diesel"
```

In practice, only one provider per interface is active at runtime (selected by config), but you can compile in multiple for flexibility.
