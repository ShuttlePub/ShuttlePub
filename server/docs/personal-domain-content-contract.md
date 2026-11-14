# S8 personal domain content contract

Issue #18 / D08. **Contract only, not enforcement.** The consumer source of truth is generated
`openapi/shuttlepub.json`, produced from
`server/src/api_v1/personal_domain.rs`. All nine new routes, and the existing S3 reaction
route, always return structured `501 not_implemented`, without extractors.
There is no actual filter, policy store, normalizer, acceptance gate or posting
implementation. Test reference projections are not production implementations.

## API inventory

All paths below begin with `/api/v1/profiles/{profile_nanoid}`; all use
`bearer_auth`. `note_id` is the existing ShuttlePub Note UUID.

| Method | Suffix | operationId | Main schemas |
| --- | --- | --- | --- |
| GET | /timeline | getPersonalTimeline | TimelineResponse, FeedItem |
| GET | /notes/search | searchPersonalNotes | NoteSearchResponse, NoteSearchItem (see "Note search") |
| GET | /notifications | getPersonalContentNotifications | ContentNotificationsResponse |
| GET | /notes/{note_id} | getPersonalNote | DirectNoteResponse, NotePresentation |
| POST | /notes/{note_id}/reveal | revealPersonalNote | DirectNoteResponse |
| POST | /notes/{note_id}/replies | createPersonalReply | PostComment, ErrorBody + ReplyDomainBlock |
| POST | /notes/{note_id}/quotes | createPersonalQuote | PostComment, ErrorBody + QuoteDomainBlock |
| POST | /notes/{note_id}/reposts | createPersonalRepost | ErrorBody + RepostDomainBlock |
| PUT | /notes/{note_id}/like | putPersonalLike | ErrorBody + LikeDomainBlock |
| PUT (existing S3) | /notes/{note_id}/reaction | putNoteReaction | PutReaction, ReactionState, ErrorBody + EmojiReactionDomainBlock |

The four new outbound operation routes define the **refusal projection only**.
S8 does not invent successful post/like/repost creation bodies or semantics.
They document 401/403/404/501, not a fabricated success. The read/reveal routes
also document future 200 representations; no stub returns them.

Inbound: `InboundReplyRejection`, `InboundQuoteRejection`,
`InboundRepostRejection`, `InboundLikeRejection`, and
`InboundEmojiReactionRejection` are **internal acceptance-decision result
contracts**, NOT an ActivityPub inbox wire protocol or HTTP-status decision.
They are registered as component schemas with separate fixtures. No new inbox
or public permission/preflight service is introduced; legacy /inbox is unchanged.

## E8 correspondence (no new policy identity or revision)

`tests/fixtures/personal_domain/e8_contract.json` pins extracted upstream schema
components from Emumet origin/main commit
`11414a68ec1bb6872d8af6c09f7e1a312c19922e`, file `openapi.json`.
The normative semantic reference is host
`intents/emumet/features/personal-domain-controls/acceptance.md`, sections
“E8 確定契約” / “D8P 確定契約”.

| E8 source | S8 projection |
| --- | --- |
| /api/v1/profiles/{profile_nanoid}/domain-controls[/{domain}] | Path profile_nanoid, response profile_id, existing S3 ProfileNanoid |
| DomainControlResponse.profile_id, ProfileResponse.id | DomainControlRef.profile_id; never Account ID, actor URL or nullable username |
| DomainControlResponse.domain | ControlDomain: already normalized; no normalization function is implemented here |
| DomainControlMode mute/block | DomainControlMode, with block-only projection for refusals |
| DomainControlResponse.revision (int64) | Same revision in hidden and control references, not a local/reveal/S3-settings revision |
| ProfileActorRef.actor_url | Matching uses verified canonical actor-ID host only; never display acct/claimed domain |

Domains use E8's UTS #46 non-transitional normalization, lowercase ASCII/IDNA
A-labels, one trailing dot removal, and exact matching (not subdomains). Invalid
input is E8's 422; S8 has no domain-input or domain-controls update route.
Revisions start at 1 and advance on create, replace, deletion and recreation;
E8 tombstones prevent reuse. Upstream's generated schema says minimum 0 while
its text and D8P semantics start at 1; the read-only S8 observation uses minimum
1 and the same signed int64 type, not another revision mechanism.

## Presentation and privacy

- Mute and block both suppress only the selected Profile's timeline, post
  search and notifications. Omit the **whole** third-party quote, including the
  quoter's comment, and the whole pure boost when the immediate original is
  suppressed. Do not auto-mute/block the quoter/booster or unrelated posts.
- FeedItem has no hidden/placeholder variant. Closed response/cursor metadata
  has no suppression count or annotation. Suppressed notifications do not
  contribute to unread_count. Visible counts are not adjusted or annotated.
- Direct hidden is exactly `state, mode, domain, revision` inside a Profile
  envelope. No note ID, body, author detail, media, attachment, CW text, mention,
  quote data or arbitrary extra property is permitted. Schema **and** serde
  reject unknown hidden fields. OpenAPI enum closure fixups run after routes!
  contributes all schemas, so the generated contract really enforces this.
- Direct GET is initially hidden. Explicit POST reveal returns only this note's
  page-local presentation. Do not persist/sync it or change E8 policy/revision;
  hide discards it, reload/revisit performs GET and hides again. Do not cache a
  revealed response across page lifetimes. There is no reusable reveal token.
- Revealing a third-party quote may show the quoter's comment while its quoted
  target remains independently hidden. The unblocked quoter remains actionable.
  CW/sensitive/deletion/viewing-rights gates remain restricted and content-free;
  domain operation restrictions also remain in those restricted responses.
- DomainOperationPermission is **domain-only**, never a grant overriding other
  permissions. Blocked direct authors remain locked after reveal; mute adds no
  operation restriction. An explicit Emumet unblock must precede a new operation.

## Note search (D04, issue #22)

`searchPersonalNotes` was extended in place (same path and operationId; there is no second
search contract). Its result type was **replaced**: `NoteSearchResponse.items` is now
`NoteSearchItem`, not `FeedItem`. This is a breaking change for any consumer pinned to the S8
generated schema (for example frontends); consuming it requires a new explicit producer SHA pin.
Everything else in S8 (timeline, notifications, FeedItem, direct notes) is unchanged. Contract only:
the route still answers 501.

- **Input.** `author` (optional, one canonical https actor ID URL, schema `ActorUrl`), `q`
  (optional, 1..=256 characters), `limit` (1..=50, default 20), `cursor`. Neither `author` nor
  `q` is `400 invalid_request`, as are an empty or over-long `q`, an out-of-range `limit`, an
  `author` that is not an actor ID URL and a bad cursor (`details.field` names the parameter).
  Author-only, keyword-only (no author, **also in the standard engine-less configuration**:
  database substring match bounded by these limits and the time limit) and both are provided.
  A search engine, when present, must not change what a result means.
- **Acct resolution is not part of this contract.** The API never accepts an acct or the UI's
  `from:@user` syntax (it is a 400). Turning `from:@user` into an actor ID URL is the BFF/UI's
  job. No existing API resolves a remote acct to an actor ID (Emumet's WebFinger only describes its
  own local accounts), so until a separate resolver contract exists the BFF/UI resolves it itself,
  and an acct it cannot resolve is **not searched**: the UI reports the error and never calls this
  route with a guess.
- **Meaning.** `q` is a literal substring of the note's body **and** its content warning; the
  text of a quoted or replied-to note is not searched. `%`, `_` and backslash are plain characters.
  Results are newest first (`created_at` descending, then `note_id` descending) and only notes the
  Profile may view. **Undecided, deliberately not fixed here:** case-insensitive matching and whether
  whitespace-separated words are ANDed.
- **Result type.** `NoteSearchItem` is `post`, `reply` or `quote` (a quote with a comment) only; a
  pure repost cannot be expressed. A note carries `note_id`, `author` (actor ID URL), `created_at` and
  a closed `body`: `{display: text, content}` or `{display: content_warning, content_warning}`. A
  content-warned note never carries its body, even when only the body matched.
- **Suppression.** S8's mute/block suppression applies to search with no trace: notes from a
  controlled domain and quotes whose direct target is in one are omitted whole. An `author` in a
  controlled domain is an ordinary empty `200`; `404 not_found` (`details.field = author`) is only for an
  actor this server has not fetched. Validation errors (400) are decided before the 404.
- **Time limit.** Reaching the response time limit **with resumable progress** returns a partial
  `200` with `page.truncated = true`: the items are an exact prefix of the full result (no gaps,
  possibly none) and `next_cursor` is always present, resuming after the last scanned position
  with a fresh time budget (the UI shows "interrupted here" and may keep auto-loading). Only a timeout
  **without any resumable progress** is `503 search_timeout`. Page metadata is closed: no total,
  hidden count or suppression note.
- **Cursor.** Opaque. Internally a `(created_at, note_id)` keyset position bound to the Profile,
  `author` and `q` (not to `limit`, not to the E8 revision). Another condition or a malformed value
  is `400` with `details.field = cursor`. `next_cursor` is present when a further match is known or
  the page is truncated; the following page may still be empty.
- **Changes between pages.** There is no snapshot: each page is evaluated against the latest state.
  An edit never changes `created_at` or the order (a note that stops matching no longer appears on
  later pages, an older note that starts matching does); a deletion, lost view right or newly added
  mute/block takes effect on the next page; nothing already returned is retracted; no note is
  returned twice.

Evidence: `tests/note_search.rs` with `fixtures/personal_domain/search.json` (27 cases and 8 paging
scenarios against a test-only reference projection), `search_schema_cases.json` (closed shapes, no
repost, CW without body, actor URL pattern, truncated page) and checks of the generated parameter
schemas. S8's `surfaces.json` now verifies only timeline and notifications; its old search
expectation (FeedItem) was replaced by the new fixtures.

## Operation refusals and S3 compatibility

Each action has a separate detail type with a singleton operation discriminator.
Like is `operation: like`; emoji is `operation: emoji_reaction` plus the same
EmojiRef as S3. Both inbound and outbound are refused for a matching block even
when D13 permits the action. Existing accepted likes/emoji stay counted; refused
new operations do not add counts. There is no automatic deletion/Undo/AP Block.

`ErrorBody` keeps its existing required `code, message` and optional `details`; the D04 unit adds the
code `search_timeout` (503).
It gains only optional `domain_block: OutboundDomainBlock`, absent on all old
errors and every 501. A block refusal uses existing `forbidden` (403), not a
parallel error format. The S3 PUT operationId, request schema, 200/4xx response
schema references and old fixtures/scenarios remain unchanged. Its 403 description
now documents this additive extension. Unblocked S3 behaviour is unchanged;
DELETE is unchanged and S8 introduces no domain-block refusal for removal.
Frontends F3 pinned to S3 commit 5945e41a remains compatible; consuming S8 requires
an explicit new producer SHA/checksum pin, not a moving branch.

## Deliberately unspecified

Boost-wrapper **direct reveal is undecided and outside this contract**; only its
whole-item feed suppression is fixed. Direct reveal of the original note URL is
ordinary single-note reveal. Also not decided here: Account discovery/lists,
nested-quote attribution, historical notification/push delivery/recall details,
or new Follow (Emumet D8G/D8R). These need later intent clarification; no schema
silently defines them. Actual enforcement is a separate future ShuttlePub unit.

## Evidence

`server/tests/personal_domain.rs` checks typed round trips, E8 field alignment
and signed revision boundaries, 77 positive/negative JSON Schema fixtures,
unknown-field/value non-leakage, per-Profile exact canonical-host projections,
whole quote/boost omission and unread counts, one-page reveal and independent
gates, ten direction/action refusal cases including D13, counts, S3 compatibility,
and all nine new routes plus S3 PUT with malformed unauthenticated requests.
Existing openapi tests cover determinism, served-byte equality, routes! single
registration, all-route 501s, and exclusion of development APIs. Existing S3
scenarios are retained (only an ErrorBody constructor gains domain_block: None).

Regenerate and run the normal workspace fmt/clippy/test/build checks described
in `docs/foundation.md`. This environment uses `cargo +1.97.1` and cached
`--offline --locked` dependencies; no repository toolchain change is needed.
