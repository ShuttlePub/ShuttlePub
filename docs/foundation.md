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
  selected (d09-q02). It is the only place where "the image is gone" is
  expressed: after the origin deleted the emoji and the local copy expired
  (d09-q03, period not part of the contract) it is `available` + `missing`
  (no `unavailable_reason`), the same look as a failed fetch, and it can be
  pressed from an existing chip (UI spec, d09-q05; decision d09-q07 changed the
  earlier `unavailable` + `deleted` form). Sendability is a separate axis:
  `availability` (an external emoji is `unavailable` only when this server
  disabled it, reason `disabled`, never `deleted`) plus the usage rules below.
  `EmojiView::check_consistency` rejects an external emoji with
  `unavailable_reason: deleted`. Local custom emoji are unchanged: deleted or
  disabled is `unavailable`.
- `usage` (`existing_reaction_only` | `anywhere`): an external emoji is
  `existing_reaction_only` (d09-q01): never in the picker, recents or
  favourites. Settings `PUT` with one is `422 emoji_usage_not_allowed` at
  `/favorites/{index}` and no settings answer lists one.
- Reaction `PUT` check order, first match answers: availability (`422
  emoji_unavailable`: unavailable local custom emoji and external emoji this
  server disabled; **not** an external emoji deleted at its origin) → same as the
  current selection (`200` no-op) → external emoji without a chip of it on the
  Note (`422 emoji_usage_not_allowed`, including one deleted at its origin) →
  external emoji while the Profile reacted with another emoji (`409 conflict`,
  no direct replacement, current reaction stays) → select (`200`). So an
  external emoji deleted at its origin whose chip is still on the Note is
  selected with `200` (the answer shows `image_state: missing` and the
  `shortcode` for `:name:`), repeating it is the `200` no-op, and without a
  chip it is `422 emoji_usage_not_allowed` (not `emoji_unavailable`). The chip
  and conflict rules only apply to `existing_reaction_only` emoji; Unicode and
  same-server custom emoji keep the replacement behaviour. `DELETE` is
  unchanged. D03 q09/q10 add nothing to any response, so no field exists for them.
- `ErrorCode` gained `emoji_usage_not_allowed` (422); `putNoteReaction` now
  documents a `409` (`ErrorBody`).

Tests: scenarios `remote_*.json` (piggy-back lifecycle, missing chip, 409,
local replacement, availability first with a disabled external emoji, missing
image, deleted-at-origin emoji reusable from its chip, never a favourite) are
replayed by the same reference model; `schema_cases.json` gained D09 response
cases; `tests/fixtures/personal_domain/remote_emoji_chips.json` checks the
per-chip origin guarantee (`EmojiReactionCount::check_consistency`).
