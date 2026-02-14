<!-- this whole file could be added as doc comments to relevant routes and have each route generate an openapi spec which we can then serve online too? not a critique of this file just a thought-->
# DNS API

This document describes the DNS record management API — endpoints, request/response formats, and how operations flow through the system.

All API routes require authentication. Include the `login-token` cookie from a successful login.
<!-- or in the future an api key -->

## Endpoints

| Method   | Path                     | Description               |
| -------- | ------------------------ | ------------------------- |
| `GET`    | `/api/dns/records`       | List user's DNS records   |
| `POST`   | `/api/dns/add_record`    | Create a new record       |
| `PUT`    | `/api/dns/update_record` | Update an existing record |
| `DELETE` | `/api/dns/delete_record` | Delete a record           |

## Authentication

All endpoints return `401 Unauthorized` if:
- No `login-token` cookie present
- Token is invalid or expired

## Rate Limiting

API routes have per-user rate limiting:
- **Limit:** 30 requests per 60 seconds (configurable)
- **Scope:** Per authenticated user
- **Response:** `429 Too Many Requests` when exceeded

## Subdomain Ownership

Users can only manage records under their subdomain. For user `alice` with public suffix `.is.fckn.gay`:

| Record Name                     | Allowed?                         |
| ------------------------------- | -------------------------------- |
| `alice.is.fckn.gay`             | ✅ User's root                    |
| `www.alice.is.fckn.gay`         | ✅ Subdomain of root              |
| `deep.nested.alice.is.fckn.gay` | ✅ Nested subdomain               |
| `bob.is.fckn.gay`               | ❌ Different user                 |
| `alice.bob.is.fckn.gay`         | ❌ Under bob's root               |
| `alice-fake.is.fckn.gay`        | ❌ Different user (partial match) |

<!-- cite relevant DNS here, link to the section that says this -->
DNS names are case-insensitive per RFC 1035 — `ALICE.is.fckn.gay` and `alice.is.fckn.gay` are equivalent.

---

## GET /api/dns/records

List all DNS records owned by the authenticated user.

### Request
<!-- authentication already discusses up, not needed here -->
No body required. Authentication via cookie.

### Response

```json
[
  {
    "id": { "0": "550e8400-e29b-41d4-a716-446655440000" },
    "provider_key": "123456",
    "record": {
      "name": "alice.is.fckn.gay",
      "record_type": "A",
      "content": "1.2.3.4",
      "ttl_seconds": 300
    }
  }
]
```

### Errors

| Status | Meaning                                          |
| ------ | ------------------------------------------------ |
| `401`  | Not authenticated                                |
| `500`  | Database error  <!-- or other internal error --> |

---

## POST /api/dns/add_record

Create a new DNS record.

### Request

```json
{
  "name": "www.alice.is.fckn.gay",
  "record_type": "A",
  "content": "1.2.3.4",
  "ttl_seconds": 300
}
```
<!-- what are defaults and optional values? if any-->
**Fields:**
- `name`: Full DNS name (must be under user's subdomain) <!-- could include a note about how it handles non-ascii names and why -->
- `record_type`: One of `A`, `AAAA`, `CNAME`, `ALIAS`, `MX`, `NS`, `TXT`, `SRV`, `TLSA`, `CAA`, `HTTPS`, `SVCB`
- `content`: Record data (IP address, domain name, text, etc.). For MX records, include priority (e.g., `"10 mail.example.com"`)
- `ttl_seconds`: Time-to-live in seconds (minimum 300)

### Response

```json
{ "0": "550e8400-e29b-41d4-a716-446655440000" }
```

Returns the new record's database ID.

### Transaction Flow
<!-- this is missing error paths for if upstream errors out and database failure could be done inline-->
```
┌──────┐         ┌──────┐         ┌────────┐         ┌────────┐
│Client│         │Server│         │Database│         │Upstream│
└──┬───┘         └──┬───┘         └───┬────┘         └───┬────┘
   │                │                 │                  │
   │ POST add_record│                 │                  │
   │───────────────>│                 │                  │
   │                │                 │                  │
   │                │ 1. validate name + ownership       │
   │                │                 │                  │
   │                │ 2. add_record ──────────────────────>
   │                │                 │                  │
   │                │                 │    provider_key  │
   │                │ <────────────────────────────────────
   │                │                 │                  │
   │                │ 3. add_dns_record(provider_key)    │
   │                │ ───────────────>│                  │
   │                │                 │                  │
   │                │     record_id   │                  │
   │                │ <───────────────│                  │
   │                │                 │                  │
   │   201 + id     │                 │                  │
   │<───────────────│                 │                  │

   On database failure:
   │                │ 4. delete_record(provider_key) ─────>  (rollback)
```
DNS provider is updated first because we need the `provider_key` it returns before we can store the record in our database. If the database write fails, we roll back by deleting from upstream.

### Errors

| Status | Meaning                                                                   |
| ------ | ------------------------------------------------------------------------- |
| `400`  | Invalid record name or not under user's subdomain                         |
| `401`  | Not authenticated                                                         |
| `502`  | DNS provider rejected the record                                          |
| `500`  | Database error (DNS provider change rolled back) <!-- or other errors --> |

---

## PUT /api/dns/update_record

Update an existing DNS record.

### Request

```json
{
  "id": { "0": "550e8400-e29b-41d4-a716-446655440000" },
  "content": {
    "name": "www.alice.is.fckn.gay",
    "record_type": "A",
    "content": "5.6.7.8",
    "ttl_seconds": 600
  }
}
```

**Note:** The `name` in `content` must still be under the user's subdomain. You can't move a record to a different subdomain.
<!-- can we move a record in _any_ situation or onlyu update the content and possibly type? -->
### Response

`204 No Content` on success.

### Transaction Flow

```
┌──────┐         ┌──────┐         ┌────────┐         ┌────────┐
│Client│         │Server│         │Database│         │Upstream│
└──┬───┘         └──┬───┘         └───┬────┘         └───┬────┘
   │                │                 │                  │
   │ PUT update     │                 │                  │
   │───────────────>│                 │                  │
   │                │                 │                  │
   │                │ 1. validate name + ownership       │
   │                │                 │                  │
   │                │ 2. get_provider_key(id)            │
   │                │ ───────────────>│                  │
   │                │                 │                  │
   │                │   provider_key  │                  │
   │                │ <───────────────│                  │
   │                │                 │                  │
   │                │ 3. update_record(key, data) ────────>
   │                │                 │                  │
   │                │                 │       ok         │
   │                │ <────────────────────────────────────
   │                │                 │                  │
   │                │ 4. update_dns_record(id, data)     │
   │                │ ───────────────>│                  │
   │                │                 │                  │
   │   204          │                 │                  │
   │<───────────────│                 │                  │

   On database failure: logged, can't rollback (don't have original data)
```
<!-- inline database failure path. make a note we might want to support roll-back here but then we also have to deal with the rollback failing. we might want to revisit this one later. -->

### Errors

| Status | Meaning                                           |
| ------ | ------------------------------------------------- |
| `400`  | Invalid record name or not under user's subdomain |
| `401`  | Not authenticated                                 |
| `404`  | Record not found or not owned by user             |
| `502`  | DNS provider rejected the update                  |
| `500`  | Database error <!-- or general?-->                |

---

## DELETE /api/dns/delete_record

Delete a DNS record.

### Request

```json
{ "0": "550e8400-e29b-41d4-a716-446655440000" }
```

Just the record ID.

### Response

`204 No Content` on success.

### Transaction Flow

```
┌──────┐         ┌──────┐         ┌────────┐         ┌────────┐
│Client│         │Server│         │Database│         │Upstream│
└──┬───┘         └──┬───┘         └───┬────┘         └───┬────┘
   │                │                 │                  │
   │ DELETE record  │                 │                  │
   │───────────────>│                 │                  │
   │                │                 │                  │
   │                │ 1. get_provider_key(id)            │
   │                │ ───────────────>│ (verifies owner) │
   │                │                 │                  │
   │                │   provider_key  │                  │
   │                │ <───────────────│                  │
   │                │                 │                  │
   │                │ 2. delete_dns_record(id)           │
   │                │ ───────────────>│                  │
   │                │                 │                  │
   │                │ 3. delete_record(provider_key) ─────>
   │                │                 │                  │
   │                │                 │       ok         │
   │                │ <────────────────────────────────────
   │                │                 │                  │
   │   204          │                 │                  │
   │<───────────────│                 │                  │

   On upstream failure: logged, can't rollback (already deleted from DB)
   → diverged state: record exists upstream but not in our DB
```
<!-- similar ot previous, want to revisist -->
Database is deleted first here because once we delete from the database, we lose the record data. If DNS deletion fails, we're left in a diverged state (record exists upstream but not in our DB). This is logged and would need manual intervention.

### Errors

| Status | Meaning                                                  |
| ------ | -------------------------------------------------------- |
| `401`  | Not authenticated                                        |
| `404`  | Record not found or not owned by user                    |
| `502`  | DNS provider deletion failed (database already deleted!) |
| `500`  | Database error <!-- same -->                             |

---

## Record Types
<!-- not needed really needed in these docs really. Only describe flow not whole api.-->
<!-- could note that some records, noteably alias, aren't supported by all providers -->
### A / AAAA

IPv4 and IPv6 address records.

```json
{
  "name": "alice.is.fckn.gay",
  "record_type": "A",
  "content": "93.184.216.34",
  "ttl_seconds": 300
}
```

### CNAME

Canonical name (alias to another domain).

```json
{
  "name": "www.alice.is.fckn.gay",
  "record_type": "CNAME",
  "content": "alice.is.fckn.gay",
  "ttl_seconds": 300
}
```

**Note:** CNAME cannot coexist with other record types at the same name.

### MX

Mail exchange record.

```json
{
  "name": "alice.is.fckn.gay",
  "record_type": "MX",
  "content": "10 mail.example.com",
  "ttl_seconds": 300
}
```

**Note:** Priority is included in `content` (e.g., `"10 mail.example.com"`) rather than as a separate field.

### TXT

Text record (often used for verification, SPF, DKIM).

```json
{
  "name": "alice.is.fckn.gay",
  "record_type": "TXT",
  "content": "v=spf1 include:_spf.google.com ~all",
  "ttl_seconds": 300
}
```

### Others

`NS`, `SRV`, `TLSA`, `CAA`, `HTTPS`, `SVCB`, `ALIAS` are supported but less common. Content format varies by type.

---

## Error Response Format

Errors return an appropriate status code with a JSON body:

```json
{
  "error": "Record not found"
}
```

Or for validation errors:

```json
{
  "error": "invalid record name: must not exceed 253 characters, labels must not exceed 63 characters"
}
```

---

## Divergence

Because we store records in both the database and the DNS provider, they can get out of sync. The API uses transaction-like behavior to minimize this:

| Operation | Order | Rollback on Failure |
|-----------|-------|---------------------|
| Add | DNS → DB | ✅ Delete from DNS if DB fails |
| Update | DNS → DB | ❌ No original data to restore |
| Delete | DB → DNS | ❌ Already deleted from DB |

When rollback isn't possible, we're left in a diverged state. For now this requires manual cleanup.

<!-- TODO: implement divergence logging/alerting so we actually know when this happens -->
