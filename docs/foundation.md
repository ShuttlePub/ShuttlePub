# Note foundation

## Run

```sh
cargo run -p shuttlepub-server
```

Defaults: `127.0.0.1:3000`, actor `http://127.0.0.1:3000/actor`.
Override `SHUTTLEPUB_BIND`, `SHUTTLEPUB_ACTOR` and comma-separated
`SHUTTLEPUB_REMOTE_ORIGINS` (empty means deny all remote access).
Use the `/actor` path on the public origin. Only explicitly trusted origins may
be listed, because they can resolve to internal addresses. Redirects and ambient
HTTP proxies are disabled. HTTP origins are for local testing; use HTTPS for
remote peers. This is an unauthenticated development service, not internet-ready.

GET `/openapi.json` serves the generated `/api/v1` contract (see "OpenAPI contract"
below); the legacy development routes are not part of it. POST `/api/notes` with
client-generated UUID `id`, UUID `author`, `content`, and `kind: {"type":"post"}`.
Other kinds require `target`: `reply`, `turbo`, `turbo_quote`. Turbo requires empty
content; all other kinds require nonblank content. Targets are opaque identifiers
(not existence-checked in this slice). One reaction value per actor is stored;
repeating it is a no-op, changing it replaces the value. GET `/api/timeline`
supports limit 1..100 and exclusive `before` creation-sequence cursor.

## Architecture and durability

- `kernel`: pure Note aggregate `decide/apply`, creation event variants and reactions.
- `application`: nitinol ProcessSystem + EventSourceSystem, JSON codec, shared
  in-memory EventStore/SnapshotStore, global Projector and timeline read model.
- `driver`: stargate-derived RSA/SHA256 draft HTTP signatures, Actor lookup,
  Follow validation and Accept delivery.
- `server`: axum REST and ActivityPub development endpoints.

nitinol is pinned to `8c634351d099910b27d26622201ced3914896fc9` because it is evolving
in parallel. Current Projector polls committed events globally (including live
events); earlier example prose saying live events are not visible is stale.
The read model is eventually consistent; command success means persisted, not
yet visible in the timeline. Creation sequence determines order; reactions do not
bump notes. Projector updates are idempotent assignments keyed by Note ID.

Event persistence and replay are within the lifetime of the supplied in-memory
stores, **not across process restarts**. Snapshot restore is wired; automatic
snapshot scheduling is not enabled. Restarting the executable loses events,
snapshots, projections and its generated RSA private key. No private keys are
written to disk or committed. The existing root PostgreSQL migration/compose
artifacts are historical and unused; no PostgreSQL connection is made.

## Federation boundary

POST `/inbox` accepts only Follow targeting this service's actor. Actor identity,
key ID and key owner must agree. SHA-256 body digest and RSA-SHA256 signatures
cover `(request-target) host date digest content-type`; date tolerance is 300s.
The remote actor document and request body are limited to 256 KiB. Successful
Accept delivery is required before responding 202. Duplicate Follow requests may
redeliver the same deterministic Accept ID; durable deduplication/outbox is not
part of this slice. Invalid signatures never cause Accept delivery.

Emumet is not called. Its future capability/signing and relay requirements are
in [emumet-contract.md](emumet-contract.md).

## Provenance

The federation algorithm is adapted from HalsekiRaika/stargate:
`driver/src/client/http.rs`, `driver/src/signature/{signer,verifier}.rs`,
`driver/src/hasher.rs`, `app-cmd/src/interactors/relay/follow_accept.rs`.
The implementation uses the same http-msgsign-draft and RSA-SHA256 primitives,
but uses typed errors, limits, allowlisted destinations and safe parser guards
instead of copying unchecked unwraps or the original dependency graph.
Upstream stargate's LICENSE is MIT, copyright (c) 2025 RechellaTek;
its manifest declares MIT OR Apache-2.0. The MIT notice is preserved in
[stargate-license.txt](stargate-license.txt).

## Verification

```sh
cargo test --workspace --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
cargo build --workspace --locked
git diff --check
```

On the implementation sandbox, the default stable toolchain's linker references
a removed Nix store path. Use `cargo +1.97.1` for every command above (including
`run`) in that environment. No repository toolchain file or linker override is
needed. This is an environment workaround, not a project or CI requirement.

### Issue #12 contract evidence

| Contract | Implementation | Executable evidence |
| --- | --- | --- |
| 1. ProcessSystem/EventSourceSystem and in-memory stores | `application/src/service.rs` | `application/tests/runtime.rs`; `server/tests/process.rs` launches the actual binary |
| 2. Post/reply/turbo/turbo_quote/reaction decide/apply and replay | `kernel/src/{note,types}.rs`, JSON codec | `kernel/tests/note.rs`; runtime tests reactivate every kind from shared event stores and restore snapshot plus event delta |
| 3. Projector timeline and REST | `application/src/timeline.rs`, `server/src/routes.rs` | `server/tests/http.rs`, `server/tests/pagination.rs`: creation, reaction, newest-first cursor pagination and invalid limits |
| 4. Follow/Accept, actor resolution and HTTPSig | `driver/src/{federation,keys,signature,wire}.rs` | `driver/tests/federation.rs`; `server/tests/federation.rs` uses real HTTP endpoints, independent RSA keys, verified Accept and tampered Follow rejection |
| 5. Emumet capability signing and post-relay requirements | `docs/emumet-contract.md` | Document review; proposal only, not an implemented or approved Emumet API |

The CI workflow runs all workspace tests (including aggregate/replay tests),
clippy with warnings denied, formatting and build. No external service or
PostgreSQL is required. Federation fixtures use ephemeral loopback ports.

## OpenAPI contract (`/api/v1`)

The server layer defines the `/api/v1` routes and types only; handlers are
stubs that answer an explicit `501` with error code `not_implemented`
(`server/src/api_v1.rs`). The OpenAPI 3.1 document is generated with utoipa from
`server/src/openapi.rs` and checked in as `openapi/shuttlepub.json`. Output is
deterministic (object keys sorted, two-space indent, trailing newline).
`GET /openapi.json` serves exactly the same content. The legacy development
routes (`/api/notes`, `/api/timeline`, ...) are intentionally not in the schema.

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
legacy development route `POST /api/notes/{id}/reactions` (one reaction value
per actor) is a separate development API: it is neither changed nor part of
this contract or of `openapi/shuttlepub.json`. The UI-facing source of truth is
the generated schema only.

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
  emoji is `422 emoji_unavailable`. `DELETE` always answers `200` with
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
