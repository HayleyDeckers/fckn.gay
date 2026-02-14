# Architecture Overview

This document describes the high-level architecture of the fckn.gay subdomain registrar — what we're building, why we made certain choices, and how the pieces fit together.

## What We're Building

A **modular subdomain registrar** that lets users claim a subdomain (like `alice.is.fckn.gay`) and manage their own DNS records through an API. Think "dynamic DNS but social" — users get their own namespace and can configure whatever records they want.

The key insight driving the architecture: **we don't want to be locked into any specific provider**. DNS providers, email services, and databases should all be swappable without touching the core server code.

## Design Goals

### What We Optimize For

- **Adaptability**: Easy to swap providers, add new ones, or run different configs per environment
- **Simplicity**: Each crate does one thing. No circular dependencies, no spaghetti.
- **Correctness**: Type-safe interfaces, proper error handling, no panics in happy paths
- **Contributor-friendly**: Add a new DNS or email provider without understanding the whole codebase. The architecture scales from single-binary deployment to HA external services.

### What We Accept

- **Some duplication**: Each implementor handles its own config/errors rather than sharing abstractions. This keeps crates independent (though we do share helpers like `secret`).
- **Feature flags**: More flags means more compile configurations, but in practice only one provider per interface is active at runtime.

## The Provider Pattern

At the heart of the architecture is a trait-based abstraction layer. Each external dependency (DNS, email, user storage) is defined as a **trait** with multiple **implementors**:

```
┌─────────────────────────────────────────────────────────┐
│                      server                             │
│  (consumes traits, doesn't care about implementations)  │
└───────────────┬─────────────────┬─────────────────┬─────┘
                │                 │                 │
         ┌──────▼──────┐   ┌──────▼──────┐   ┌──────▼──────┐
         │  Dns trait  │   │ Email trait │   │ UserDatabase│
         └──────┬──────┘   └──────┬──────┘   └──────┬──────┘
                │                 │                 │
        ┌───────┼───────┐    ┌────┼────┐     ┌──────┼──────┐
        ▼       ▼       ▼    ▼         ▼     ▼      ▼      ▼
     Dummy  Hickory  Porkbun Dummy   Lettre  Dummy  CSV  Diesel
```

### Why This Pattern?

1. **No vendor lock-in**: Switch from Porkbun to self-hosted Hickory DNS by changing config
2. **Easy testing**: Use `Dummy` implementations that store in-memory or print to stdout
3. **Clear boundaries**: Each provider is its own crate — like microservices in a single binary

This maps to the workspace like so:

```
this-workspace-is-fckn-gay/
├── server/           # Axum web server (the only runnable)
├── dns/              # DNS trait + implementors (dummy, hickory, porkbun)
├── email/            # Email trait + implementors (dummy, lettre)
├── user-database/    # Storage trait + implementors (dummy, csv, diesel)
├── validation/       # Shared validation (compiles to WASM too)
└── secret/           # Load secrets from env, file, or inline
```

For implementation details on each crate, see [implementation.md](implementation.md).

## Data Ownership & Source of Truth

```
┌────────┐      ┌────────────────────────────────────────┐      ┌──────────┐
│        │      │               server                   │      │          │
│ Client │ ───> │  routes ──> handlers ──> providers     │ ───> │ Upstream │
│        │      │                │                       │      │   DNS    │
└────────┘      │                │                       │      └──────────┘
                │                ▼                       │
                │           ┌─────────┐                  │
                │           │Database │                  │
                │           │(source  │                  │
                │           │of truth)│                  │
                │           └─────────┘                  │
                └────────────────────────────────────────┘
```

The **database is our source of truth** for ownership and record data. Upstream DNS providers are updated to match our database state.

API responses come from the database, not upstream — this is faster but means we can serve stale data if upstream and database diverge. See [flows/api.md](flows/api.md) for how we handle divergence.

For detailed operation flows (add/update/delete), see [flows/api.md](flows/api.md).

## Further Reading

**Provider interfaces** — how each trait is designed:
- [providers/dns.md](providers/dns.md) — DNS trait, record types, the `Key` abstraction
- [providers/email.md](providers/email.md) — Email trait (it's simple)
- [providers/database.md](providers/database.md) — User storage, state management, DNS record ownership

**User-facing flows** — how requests move through the system:
- [flows/auth.md](flows/auth.md) — Signup, login, sessions, account lifecycle
- [flows/api.md](flows/api.md) — DNS record CRUD, request/response formats, divergence handling
