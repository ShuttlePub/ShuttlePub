# Emumet connection contract (proposal, not implemented)

## Status and boundary

Issue #12 names Emumet ADR 0002/0003 amended 2026-08-24 as the design baseline.
Their text is not present in the current Emumet main tree or the available local
clone. This document therefore records **ShuttlePub's requested interface**, not
a claim that Emumet already exposes these endpoints or has approved this shape.
Confirm this proposal against those amended ADRs before implementing a connection.

Emumet owns accounts, profile signing keys, capabilities and federation relay.
Ory owns authentication/authorization; Booskiff owns files. ShuttlePub owns Note
events and timeline projections. None of these services is contacted in this slice.
The development RSA key is ephemeral and must never become an Emumet credential.

## Capability issuance and signing

An authenticated, authorized link workflow should issue a short-lived capability
bound to all of `profile_id`, `link_id`, `scopes`, audience, issuer, expiry and a
revocable capability identifier. `link_id` refers to the active `shuttlepub-link`;
it is not interchangeable with `profile_id`. Initial requested scope: `sign:post`.
Issuance is an Emumet responsibility; no unrestricted signing endpoint is requested.

Proposed signing request:

```http
POST /internal/v1/capabilities/sign-post
Authorization: Bearer <capability>
Idempotency-Key: <delivery-id>
Content-Type: application/json
```

```json
{
  "profile_id": "profile-id",
  "link_id": "shuttlepub-link-id",
  "scopes": ["sign:post"],
  "request": {
    "method": "POST",
    "target_uri": "https://peer.example/inbox",
    "headers": {"host": "peer.example", "date": "<HTTP-date>", "content-type": "application/activity+json"},
    "body_base64": "<exact serialized ActivityPub bytes>"
  }
}
```

Emumet must validate the capability's signature, expiry, audience, revocation,
active link and profile binding; request `scopes` do not grant authority. A
capability for one profile/link must not sign for another. `sign:post` authorizes
only bounded, policy-checked POST activities on behalf of that profile, not arbitrary
HTTP methods or a generic signing oracle. The activity actor must match the profile.
Emumet calculates/checks the Digest over the supplied exact bytes and returns the
key ID and signed headers (`Signature`, `Digest`, `Date`), never the private key.
The caller must not reserialize the body or alter covered headers after signing.

Requested response: `200 {"key_id":"...","headers":{"Signature":"...",
"Digest":"SHA-256=...","Date":"..."},"expires_at":"..."}`.
Errors: 400 malformed activity/target, 401 expired/invalid capability, 403 wrong
profile/link/scope or revoked link, 409 idempotency-key reuse with different bytes,
429 throttled with Retry-After, 5xx transient key service failure. Do not retry
401/403 unchanged. Retrying identical bytes under the same idempotency key must
not produce a second logical delivery.

## Post-relay ingress requested from Emumet

Proposed `POST /internal/v1/post-relay` accepts a validated envelope containing
`delivery_id`, `profile_id`, `link_id`, `activity_id`, `activity_body_base64`,
`recipient_actor_ids`, and a capability token (Authorization header, never stored
in the envelope). Whether relay accepts `sign:post` or a separate `relay:post`
scope remains an **Emumet approval decision**; do not silently broaden `sign:post`.

The ingress must verify link/profile/actor binding, enforce size and recipient
limits, validate recipient URLs with SSRF protections, deduplicate by delivery ID
and body hash, and durably enqueue before returning 202 with `delivery_id` and a
status resource URI. Same ID/same content returns the original receipt; same
ID/different content returns 409. Emumet resolves destinations, signs each exact
outbound request with that profile's key, and owns bounded retries/backoff,
Retry-After handling, terminal failures and delivery-status reporting.

ShuttlePub will persist the Note before requesting relay and keep a durable
outbox keyed by `delivery_id` in the future integration slice. HTTP timeout is
an unknown result: query status or retry with the same ID, never invent a new ID.
Permanent 4xx must surface to the caller/operator without deleting Note events.
No Create/Note federation send/receive, relay ingress, capability issuance or
outbox is implemented by this foundation PR.

## Decisions still requiring agreement

- Confirm endpoint names, token format, audience and TTL against amended ADRs.
- Define profile-ID representation and translation from the development UUID actor ID.
- Confirm separate relay scope, maximum payload/recipients and delivery retry horizon.
- Define status polling/callback authentication and capability revocation propagation.
- Define canonicalization/HTTPSig version negotiation and key rotation behavior.
