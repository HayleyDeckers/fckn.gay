# Authentication Flows

This document describes how users authenticate with the system — signup, login, logout, and the account lifecycle.

## Overview

Authentication is session-based using HTTP-only cookies. Sessions are stored in an in-memory cache (cleared on server restart). Passwords are hashed with `password-auth` (Argon2).

**Routes:**
- `POST /login` — authenticate and get session
- `GET /logout` — invalidate session
- `POST /signup` — create new account (pending confirmation)
- `GET /confirm-signup/{uuid}` — activate account

## User States

Users progress through states:

```
(none) → Pending → Active
                 ↘ Inactive
                 ↘ Banned
```

- **Pending**: Account created but email not confirmed. Cannot log in.
- **Active**: Email confirmed, can log in and use the service.
- **Inactive**: Soft-deleted or voluntarily deactivated. (Planned)
- **Banned**: Suspended by admin. Cannot log in. (Planned)

Only `Active` users can authenticate.

## Signup Flow

```
┌──────┐          ┌──────┐          ┌────────┐          ┌───────┐
│Client│          │Server│          │Database│          │ Email │
└──┬───┘          └──┬───┘          └───┬────┘          └───┬───┘
   │                 │                  │                   │
   │ POST /signup    │                  │                   │
   │ {user,pass,email}                  │                   │
   │────────────────>│                  │                   │
   │                 │                  │                   │
   │                 │ is_available?    │                   │
   │                 │─────────────────>│                   │
   │                 │                  │                   │
   │                 │ validate username/password           │
   │                 │ (shared validation crate)            │
   │                 │                  │                   │
   │                 │ add_user (Pending)                   │
   │                 │─────────────────>│                   │
   │                 │                  │                   │
   │                 │      uuid        │                   │
   │                 │<─────────────────│                   │
   │                 │                  │                   │
   │                 │ send confirmation email              │
   │                 │─────────────────────────────────────>│
   │                 │                  │                   │
   │   201 Created   │                  │                   │
   │<────────────────│                  │                   │
```

**Validation rules** (from `validation` crate):
- Username: length limits, allowed characters, not reserved
- Password: minimum length, complexity requirements

**Error responses:**
- `409 Conflict` — username already taken
- `422 Unprocessable Entity` — validation failed (with error details)
- `500 Internal Server Error` — database or email error

**Known issues:**
- Race condition between `is_available` check and `add_user` — two concurrent signups could both pass the check <!-- add_user should therefor enforce uniqueness constraints. is this handled/documented in the code/databases?-->
- If email fails to send, user is left in Pending state forever (TODO: rollback or retry <!-- or timeout-->?)
- <!-- what is the behaviour of the email? if the call succeeds is it send or enqueued? -->
- Confirmation URL is hardcoded to `127.0.0.1:8080` (needs config) <!-- shouldn't be a doc but a todo for us. easy one we can just fix that to default to the bound address real quick !-->

## Email Confirmation Flow

```
┌──────┐          ┌──────┐          ┌────────┐
│Client│          │Server│          │Database│
└──┬───┘          └──┬───┘          └───┬────┘
   │                 │                  │
   │ GET /confirm-signup/{uuid}         │
   │────────────────>│                  │
   │                 │                  │
   │                 │ activate_user    │
   │                 │─────────────────>│
   │                 │                  │
   │                 │ (Pending→Active) │
   │                 │<─────────────────│
   │                 │                  │
   │ 302 Redirect /  │                  │
   │<────────────────│                  │
```

The UUID in the confirmation link is the user's database ID. Knowing it activates the account.

**Security note:** The UUID serves as both identifier and activation token. This is simple but means the UUID must be unguessable. We use UUID v4 (random).

## Login Flow

```
┌──────┐          ┌──────┐          ┌────────┐     ┌─────────┐
│Client│          │Server│          │Database│     │AuthCache│
└──┬───┘          └──┬───┘          └───┬────┘     └────┬────┘
   │                 │                  │               │
   │ POST /login     │                  │               │
   │ {user, pass}    │                  │               │
   │────────────────>│                  │               │
   │                 │                  │               │
   │                 │ validate_and_get_user_id         │
   │                 │─────────────────>│               │
   │                 │                  │               │
   │                 │ (checks Active + password)       │
   │                 │<─────────────────│               │
   │                 │                  │               │
   │                 │ new_token_for(user_id, 4h)       │
   │                 │─────────────────────────────────>│
   │                 │                  │               │
   │                 │                 token            │
   │                 │<─────────────────────────────────│
   │                 │                  │               │
   │ Set-Cookie:     │                  │               │
   │ login-token=... │                  │               │
   │ (HTTP-only)     │                  │               │
   │<────────────────│                  │               │
```

**Session tokens:**
- 128-bit random hex string (32 characters)
- Stored in-memory in `AuthenticationCache`
- Expires after 4 hours <!-- should be configurable -->
- HTTP-only cookie (not accessible to JavaScript)
- <!-- since we are a subdomain host we must be careful that evil.alice.is.fckn.gay does not read any is.fckn.gay cookies. is this ensured?-->

**Error responses:**
- `401 Unauthorized` — invalid credentials or inactive account

**Known issues:**
- Cookie domain hardcoded to `127.0.0.1` (needs config)
- Sessions lost on server restart (in-memory only)
- No "remember me" option (always 4h expiry) <!-- this is "remember me", the issue is not being able to compeletely disable it -->
- No session limit per user (can have unlimited active sessions) <!-- oh. We should make an issue for this-->

## Logout Flow

```
┌──────┐          ┌──────┐          ┌─────────┐
│Client│          │Server│          │AuthCache│
└──┬───┘          └──┬───┘          └────┬────┘
   │                 │                   │
   │ GET /logout     │                   │
   │ Cookie: token   │                   │
   │────────────────>│                   │
   │                 │                   │
   │                 │ remove_token      │
   │                 │──────────────────>│
   │                 │                   │
   │ Set-Cookie:     │                   │
   │ login-token=    │                   │
   │ (clear cookie)  │                   │
   │                 │                   │
   │ 302 Redirect /  │                   │
   │<────────────────│                   │
```

Logout is idempotent — calling it without a valid session still clears the cookie and redirects.

## Session Validation (Middleware)

Protected routes use middleware to check authentication:

```rust
// Returns 401 if not authenticated
.layer(middleware::from_fn_with_state(
    state.clone(),
    add_authorization_or_unauthorized,
))

// Redirects to / if not authenticated
.layer(middleware::from_fn_with_state(
    state.clone(),
    redirect_if_unauthorized,
))
```

The middleware:
1. Extracts `login-token` cookie
2. Looks up token in `AuthenticationCache`
3. If valid and not expired, adds `AuthenticatedFor` to request extensions
4. If expired, lazily removes the token from cache

Route handlers can then extract `AuthenticatedFor` to get the user ID:

```rust
async fn my_handler(auth: AuthenticatedFor) -> impl IntoResponse {
    let user_id = auth.user_id();
    let username = auth.username();
    // ...
}
```

## Rate Limiting

Auth routes have IP-based rate limiting to prevent brute force attacks:

- **Limit:** 10 requests per 60 seconds (configurable)
- **Scope:** Per IP address
- **Response:** `429 Too Many Requests` when exceeded

See `rate_limit.rs` for implementation.

## Planned Features

### Password Reset

Not yet implemented. Planned flow:

1. User requests <!-- password --> reset via email
2. Server generates time-limited reset token
3. Email contains link with token
4. User clicks link, enters new password
5. Token is invalidated after use

Needs:
- Reset token storage (database or cache)
- Reset email template
- Password change endpoint

### Account Deletion

Not yet implemented. Considerations:

- **Soft delete** (set state to Inactive) vs **hard delete** (remove from DB)
- What happens to user's DNS records?
- Grace period before permanent deletion?
- Re-registration with same username?

### Session Persistence

Currently sessions are in-memory only. Options for persistence:

- Store sessions in database (survives restart)
- Use signed JWT tokens (stateless, no storage needed)
- Redis/external session store (scales horizontally)

### Multi-factor Authentication

Not planned for MVP, but would be nice eventually:
- TOTP (authenticator app)
- WebAuthn (hardware keys)
- Email OTP (already have email infra)
