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

GET `/openapi.json` serves the checked-in API contract. POST `/api/notes` with
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
| 3. Projector timeline and REST | `application/src/timeline.rs`, `server/src/routes.rs`, `docs/openapi.json` | `server/tests/http.rs`, `server/tests/pagination.rs`: creation, reaction, newest-first cursor pagination and invalid limits |
| 4. Follow/Accept, actor resolution and HTTPSig | `driver/src/{federation,keys,signature,wire}.rs` | `driver/tests/federation.rs`; `server/tests/federation.rs` uses real HTTP endpoints, independent RSA keys, verified Accept and tampered Follow rejection |
| 5. Emumet capability signing and post-relay requirements | `docs/emumet-contract.md` | Document review; proposal only, not an implemented or approved Emumet API |

The CI workflow runs all workspace tests (including aggregate/replay tests),
clippy with warnings denied, formatting and build. No external service or
PostgreSQL is required. Federation fixtures use ephemeral loopback ports.
