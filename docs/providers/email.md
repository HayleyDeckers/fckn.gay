# Email Provider Interface

This document describes the `Email` trait — what it does now, what we want it to do eventually, and the design considerations around email delivery.

## The Trait

```rust
pub trait Email {
    type Config: serde::de::DeserializeOwned;
    type Error: std::error::Error + Send + Sync + 'static;

    fn new(config: Self::Config) -> Result<Self, Self::Error>;

    fn send_email(
        &self,
        from: &str,
        to: &str,
        subject: &str,
        body: &str,
    ) -> impl Future<Output = Result<(), Self::Error>>;
}
```

That's it. One method. It's simple on purpose.

## Current Semantics

### What `Ok(())` Means

**Currently:** The upstream mail server accepted the message for delivery.

This is *not* a guarantee that:
- The email will be delivered
- The recipient address exists
- The email won't bounce
- The email won't land in spam

Email delivery is inherently asynchronous and unreliable. Bounces can arrive hours later (or never). We accept this limitation for now.

### Error Handling

`Err` is returned when:
- SMTP connection fails
- Authentication fails
- Server rejects the message immediately

### Known Issues

- **Panics on invalid addresses**: The Lettre implementation calls `.expect()` on address parsing. This should return `Err` instead. (TODO)
- **No retry logic**: If sending fails, we fail immediately. No backoff, no retry.
- **No rate limiting**: We rely on the SMTP server to reject us if we're too fast.

## Current Implementors

### Dummy (`email/implementors/dummy/`)

Prints emails to stdout. Useful for development and testing. Always succeeds.

### Lettre (`email/implementors/lettre/`)

Real SMTP via the Lettre crate. Connects to configured SMTP server with TLS and authentication.

**Config:**
```toml
[email.lettre]
smtp_server = "mail.example.com"
smtp_port = 587
username = { env = "SMTP_USER" }
password = { env = "SMTP_PASS" }
```

## Future Work: Email Queue

The current fire-and-forget model works for MVP, but we want something more robust. The vision:

### Queued Delivery

Instead of sending synchronously, the server would:
1. Enqueue the email (write to database)
2. Return immediately to the user
3. Background worker processes the queue

This decouples "user requested email" from "email actually sent", improving perceived latency and reliability.

### Delivery Tracking

Each email would have a status lifecycle:

```
pending → sending → sent
                  ↘ failed → retry → sent
                           ↘ bounced
```

We'd track:
- `status`: pending, sending, sent, failed, bounced
- `retry_count`: how many times we've tried
- `next_retry_at`: when to try again (exponential backoff)
- `sent_at`: when it actually went out
- `error`: last error message if failed

### Open/Click Tracking (Maybe)

If we get fancy:
- Track email opens (via tracking pixel)
- Track link clicks (via redirect URLs)
- Webhooks from providers (SendGrid, Postmark, etc.)

This is nice-to-have, not essential.

### Database Implications

The email queue needs persistence. This ties into the database provider design — we'll need:
- An `emails` table/collection
- Methods on `UserDatabase` or a separate `EmailQueue` trait
- Background task infrastructure

See [database.md](database.md) for more on this (when we flesh it out).

## Implementing a New Provider

1. Create a new crate at `email/implementors/your-provider/`
2. Define your `Config` struct
3. Implement the trait
4. Add feature flag in `email/Cargo.toml`
5. Re-export from `email/src/lib.rs`

### Checklist

- [ ] `new()` validates config and creates client/connection pool
- [ ] `send_email()` handles connection errors gracefully
- [ ] Invalid addresses return `Err`, not panic
- [ ] Consider connection pooling for performance

## Design Decisions

### Why No `from` in Config?

The `from` address is passed per-call rather than configured once. This allows:
- Different "from" addresses for different email types (noreply@ vs support@)
- Flexibility for multi-tenant setups later

### Why Plain Text Only?

The trait only supports `body: &str` (plain text). No HTML, no attachments. This is intentional:
- Simpler interface
- Most of our emails are transactional (confirmation codes, etc.)
- HTML email is a nightmare of compatibility quirks

If we need HTML later, we can add `send_html_email()` or a builder pattern.

### Why Not a Queue Trait Now?

We could define the queue interface now, but:
- YAGNI — current use cases work with fire-and-forget
- Queue design depends on database design (chicken/egg)
- Better to build the simple thing, learn from it, then design the queue

We'll revisit when email reliability becomes a pain point.
