# Foundation

## Run

```sh
cargo run -p shuttlepub-server
```

Defaults: `127.0.0.1:3000`, actor `http://127.0.0.1:3000/actor`.
Override `SHUTTLEPUB_BIND`, `SHUTTLEPUB_ACTOR` and comma-separated
`SHUTTLEPUB_REMOTE_ORIGINS` (empty means deny all remote access; empty entries
are skipped, each entry is reduced to its origin, and an entry that is not a
URL stops the server at startup). Use the `/actor` path on the public origin.
Only explicitly trusted origins may be listed, because they can resolve to
internal addresses. Redirects and ambient HTTP proxies are disabled. HTTP
origins are for local testing; use HTTPS for remote peers. This is an
unauthenticated development service, not internet-ready.

The server answers `POST /inbox` and `GET /actor` (ActivityPub, below),
`GET /openapi.json` (the generated `/api/v1` contract) and the `/api/v1`
contract stubs. Nothing is stored: the signing key is generated in memory at
every start and is never written to disk; no database is used.

## Architecture

Four crates, depending only downwards:

| Crate | Holds | Workspace dependencies |
| --- | --- | --- |
| `kernel` | Entities and value objects (`LocalActor`, `RemoteActor`, `Follow`, `Accept`, ids, `PublicKey`), the ports to external systems and `KernelError` | none |
| `application` | Services, the settings port (`AppConfig`) and `ApplicationError` | `kernel` |
| `driver` | Adapters implementing the kernel ports (remote HTTP client, HTTP signatures, the in-memory signing key, the local actor), loading the settings, `DriverError` | `kernel` |
| `server` | The composition root (`Handler`, `AppModule`), axum routes and middleware, the HTTP status mapping, the `/api/v1` contract and `main` | `application`, `driver`, `kernel` |

`kernel` does not depend on tokio, axum or reqwest, and its types are never
used as JSON: `server` and `driver` convert at their boundary with their own
serde types.

### Dependency injection (cake pattern)

- Every port is a trait in `kernel::interface` paired with a `DependOn*`
  trait that hands out an implementation through an accessor, e.g.
  `RemoteActorResolver` / `DependOnRemoteActorResolver::remote_actor_resolver()`.
  Ports: `RemoteActorResolver`, `ActivityDelivery`, `HttpSignatureVerifier`,
  `LocalActorProvider`.
- A service in `application::services` is a trait whose supertraits are the
  `DependOn*` traits it needs and whose behaviour is in default methods; a
  blanket `impl<T: ...> Service for T {}` gives it to every type that provides
  those dependencies, and a `DependOn*Service` trait exposes it. Services have
  no struct and no fields: `AcceptFollowService`, `LocalActorService`.
- `server::app::Handler` owns the driver adapters and implements every
  `DependOn*` trait; for services it answers `type FooService = Self`. The axum
  state is `AppModule(Arc<Handler>)` (`Clone`, `Deref<Target = Handler>`), and
  handlers call services only through accessors, e.g.
  `app.accept_follow_service().accept(..)`.
- Async port methods are declared as
  `fn f(&self, ..) -> impl Future<Output = ..> + Send` and implemented with
  `async fn`; service default methods return `impl Future + Send` with one
  `async` block. No `async_trait`.
- To test a service, implement the `DependOn*` traits on a test type with fake
  ports; `application/tests/accept_follow.rs` does this without any HTTP
  server or network.

### Errors

Each layer has a `thiserror` enum carried in an `error_stack::Report`
(`KernelError`, `ApplicationError`, `DriverError`, `ServerError`). Adapters
report request-time failures through the ports as one of the `KernelError`
classes, so `application` never sees driver types; `ApplicationError` keeps
the class. Only `server/src/error.rs` maps classes to HTTP statuses: invalid
signature and untrusted identity 401, origin not allowed 403, malformed 400,
remote failure 502, internal 500; error bodies are `{"error": <reason>}`.

## Federation boundary

`POST /inbox` accepts only a Follow of this service's actor and answers `202`
once the Accept was delivered. The inbox middleware
(`server/src/routing/inbox.rs`) checks, and the first failing check decides
the answer:

1. Body larger than 256 KiB: 413.
2. `Signature` header missing or malformed (duplicated item, unquoted value,
   non-Base64 signature, invalid header name) or not covering all of
   `(request-target) host date digest content-type`: 401.
3. Body not a Follow (invalid JSON, other `type`, missing or non-string
   `id`/`actor`/`object`): 400.
4. `AcceptFollowService::identify`: Follow `id` not a URL 400; `object` not
   exactly this actor's id 401; `Host` not this actor's host and port 401;
   `actor` not a URL 400; then the actor is resolved (origin not allowed,
   non-http(s) scheme or credentials in the URL 403; fetch failure, error
   status or document over 256 KiB 502; invalid document 400; document `id`
   not the requested actor or key `owner` not the actor 401; inbox not a URL
   400 or not allowed 403); `keyId` not the actor's key id 401.
5. Signature verification: algorithm not `rsa-sha256` 401; `Date` missing,
   unreadable or more than 300 seconds away 401; unreadable public key 502;
   SHA-256 digest mismatch 401; signature mismatch 401.

The middleware is attached to the `POST` handler only, so other methods on
`/inbox` answer `405` without reading the body or contacting a remote.

The Accept (`{actor}/accepts/{Follow id}`, the Follow as `object`) is signed
with `rsa-sha256` over the same five components under keyId
`{actor}#main-key`; a delivery error or error status is 502. Remote HTTP times
out after 10 s (3 s to connect). Duplicate Follow requests redeliver the same
Accept id; deduplication, follower storage and retries are not implemented.
`GET /actor` serves the actor as `application/activity+json` (`Application`
with `id`, `inbox` and `publicKey`) without a signature.

Emumet is not called. Its future capability/signing and relay requirements are
in [emumet-contract.md](emumet-contract.md).

## Provenance

The layering and the `DependOn*` dependency injection follow
HalsekiRaika/architectured. The inbox middleware, the async port style and the
error-stack reports follow HalsekiRaika/stargate, and the HTTP signature
handling is adapted from its `driver/src/hasher.rs`, `driver/src/client/http.rs`
and `driver/src/signature/{signer,verifier}.rs`: the same http-msgsign-draft and
RSA-SHA256 primitives and signed components, with typed errors, size limits,
allowlisted destinations and a strict `Signature` header check instead of
unchecked unwraps. Upstream stargate's LICENSE is MIT, copyright (c) 2025
RechellaTek; its manifest declares MIT OR Apache-2.0. The MIT notice is
preserved in [stargate-license.txt](stargate-license.txt).

## Verification

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo build --workspace --locked
git diff --check
```

CI (`.github/workflows/lint-and-test.yml`) runs these and the `openapi-schema`
job on pushes and pull requests to `main` and `dev`. No external service is
required; federation tests start their remote peers on ephemeral loopback ports.
`server/tests/federation.rs` covers the inbox checks above end to end (test
names `f01_`..`f12_` and `order_`), `server/tests/process.rs` starts the binary.

## OpenAPI contract (`/api/v1`)

The server layer defines the `/api/v1` routes and types only; handlers are
stubs that answer an explicit `501` with error code `not_implemented`
(`server/src/api_v1.rs`). The OpenAPI 3.1 document is generated with utoipa from
`server/src/openapi.rs` and checked in as `openapi/shuttlepub.json`. Output is
deterministic (object keys sorted, two-space indent, trailing newline).
`GET /openapi.json` serves exactly the same content. Only `/api/v1` routes are
in the schema; `/inbox`, `/actor` and `/openapi.json` are not.

Regenerate after changing any route or DTO, and commit the result:

```sh
cargo run --locked -p shuttlepub-server --bin openapi
```

`server/tests/openapi.rs` fails when the checked-in file differs from the
generated output, and the `openapi-schema` CI job regenerates the file and fails
on any `git diff`. Routes have a single registration point: write a
`#[utoipa::path]` handler in `api_v1` and add it to `api_v1::router()` with
`routes!(handler)`. The macro takes the method and path from the attribute and
feeds both the axum router and the OpenAPI paths, so a route cannot exist
without its schema entry (register it any other way, e.g. a plain `.route(`, and
`server/tests/openapi.rs` fails). Only new schema types that no operation
references need an extra entry in `ApiDoc` (`components(schemas(...))`). Then
regenerate and update the snapshot expectations in `server/tests/openapi.rs`.
Stub handlers take no extractors so they answer `501` for any request. Errors
reported by handlers use `ErrorBody` (`code`, `message`, optional `details`);
framework-level 404/405 responses do not.

## Quick reaction contract (`/api/v1`)

Contract for decision D03 (favourite emoji at the top of the reaction picker,
one reaction per Profile and Note). **Contract only: nothing is stored or
synchronised yet.** All routes below are explicit `501 not_implemented` stubs
(`server/src/api_v1/quick_reaction.rs`); the `200`/`4xx` answers documented in
the schema describe the behaviour a future implementation must follow. The
UI-facing source of truth is the generated schema only.

| Method | Path | operationId | Request / success body |
| --- | --- | --- | --- |
| GET | `/api/v1/profiles/{profile_nanoid}/quick-reaction-settings` | `getQuickReactionSettings` | - / `QuickReactionSettings` |
| PUT | `/api/v1/profiles/{profile_nanoid}/quick-reaction-settings` | `putQuickReactionSettings` | `UpdateQuickReactionSettings` / `QuickReactionSettings` |
| GET | `/api/v1/profiles/{profile_nanoid}/notes/{note_id}/reaction` | `getNoteReaction` | - / `ReactionState` |
| PUT | `/api/v1/profiles/{profile_nanoid}/notes/{note_id}/reaction` | `putNoteReaction` | `PutReaction` / `ReactionState` |
| DELETE | `/api/v1/profiles/{profile_nanoid}/notes/{note_id}/reaction` | `deleteNoteReaction` | - / `ReactionState` |

Identifiers: `profile_nanoid` is Emumet's public Profile identifier (its
`/api/v1/profiles/{profile_nanoid}` routes use the same value; default nanoid
alphabet `A-Za-z0-9_-`, 1..=64 characters). `note_id` is the ShuttlePub `NoteId`
UUID. Emoji are `EmojiRef`: `{"type":"unicode","value":"👍"}` or
`{"type":"custom","id":"<stable id>"}`; the server answers `EmojiView`
(identity + `availability` + optional `unavailable_reason`, `shortcode`,
`image_url`). All five operations use the `bearer_auth` HTTP bearer scheme.

Semantics (also in the `quick_reaction` module docs and the operation texts):

- Favourites are an ordered list of at most 5 pairwise distinct emoji, shared by
  all devices of the Profile. A Profile that never saved reads as revision `0`
  with no favourites (no server-managed defaults).
- Settings `PUT` replaces the whole list and carries `expected_revision`. If the
  list equals the stored one the stored settings are returned with `200` and
  the revision is not bumped (so a retry after a lost response succeeds with the
  same result); otherwise a stale revision is `409 revision_conflict` with
  `current_revision`. Six or more favourites, duplicates and malformed
  identifiers are `400 invalid_request` with JSON-pointer `details`.
- A favourite that became unavailable is kept and flagged (never deleted or
  substituted). `PUT` may keep it (reordering) but cannot newly add an
  unavailable emoji: `422 emoji_unavailable`.
- Reaction `PUT` is idempotent and replaces a different previous emoji (the
  picker UI deletes first, but replacement is part of the API); an unavailable
  emoji is `422 emoji_unavailable`, checked before the repeat no-op rule (repeating
  the already selected emoji after it became unavailable is also `422`). `DELETE` always answers `200` with
  `emoji: null`, also when nothing was selected and **also when the selected
  emoji is unavailable**. Reaction requests carry no revision (last write wins).
- Other errors: `401 unauthorized`, `403 forbidden` (caller may not act as the
  Profile), `404 not_found` (unknown Profile or Note), always the common
  `ErrorBody`. `ErrorCode` gained `revision_conflict` (409) and
  `emoji_unavailable` (422) for this contract.

Tests: `server/tests/quick_reaction.rs` covers type serialization/validation,
schema constraints (`maxItems: 5`, `uniqueItems`), fixtures and the explicit
`501` for every route. `tests/fixtures/quick_reaction/schema_cases.json` holds
schema valid/invalid payloads (0/5/6 favourites, duplicates, unavailable,
conflict, errors) and `scenarios/*.json` hold ordered request/response
sequences (retry, conflict, unavailable favourite, deleting an unavailable
reaction). Every payload is validated against the generated schema, and the
scenarios are replayed against a small reference model of the semantics above
that lives in the test only.

### External custom emoji (D09, `remote-emoji-reuse-api-contract`)

Extends the contract above for reactions made with a custom emoji of another
server. Still contract only (`501` stubs): nothing is fetched, cached or
federated. `EmojiRef` is unchanged; the extra facts are optional `EmojiView`
fields and an optional `view` on the S8 `EmojiReactionCount` chip
(`personal_domain.rs`):

- `origin` (same canonical host form as the E8 `ControlDomain`): present
  exactly for external emoji. Because the server always sends `view` for custom
  emoji chips, the UI decides the globe mark per chip from `view.origin`;
  same-named emoji of different servers are different chips (different `emoji`
  id and `origin`).
- `image_state` (`present` | `missing`), independent of `availability`: a
  missing image is shown as `:shortcode:` and an `available` emoji can still be
  selected (d09-q02); after the origin deleted it and the local copy expired
  (d09-q03, period not part of the contract) it is `unavailable` + `missing`.
- `usage` (`existing_reaction_only` | `anywhere`): an external emoji is
  `existing_reaction_only` (d09-q01): never in the picker, recents or
  favourites. Settings `PUT` with one is `422 emoji_usage_not_allowed` at
  `/favorites/{index}` and no settings answer lists one.
- Reaction `PUT` check order, first match answers: availability (`422
  emoji_unavailable`) → same as the current selection (`200` no-op) → external
  emoji without a chip of it on the Note (`422 emoji_usage_not_allowed`) →
  external emoji while the Profile reacted with another emoji (`409 conflict`,
  no direct replacement, current reaction stays) → select (`200`). The chip and
  conflict rules only apply to `existing_reaction_only` emoji; Unicode and
  same-server custom emoji keep the replacement behaviour. `DELETE` is
  unchanged. D03 q09/q10 add nothing to any response, so no field exists for them.
- `ErrorCode` gained `emoji_usage_not_allowed` (422); `putNoteReaction` now
  documents a `409` (`ErrorBody`).

Tests: scenarios `remote_*.json` (piggy-back lifecycle, missing chip, 409,
local replacement, availability first, missing image, never a favourite) are
replayed by the same reference model; `schema_cases.json` gained D09 response
cases; `tests/fixtures/personal_domain/remote_emoji_chips.json` checks the
per-chip origin guarantee (`EmojiReactionCount::check_consistency`).
