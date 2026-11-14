# Note search author lookup contract

Issue #26 / D04 (unit `note-search-author-lookup-api-contract`, depends on #22 / PR #23).
**Contract only, UI first.** The consumer source of truth is the generated
`openapi/shuttlepub.json`, produced from `server/src/api_v1/search_author.rs`. Both routes always
answer a structured `501 not_implemented` from handlers without extractors, so invalid input and
missing credentials are never answered with `400`, `401` or a success either. Nothing here reads a
database, calls another server, schedules background work or talks to an authentication service;
the test-only reference projection (`tests/note_search_author.rs`, `tests/fixtures/note_search_author/`)
is not a production implementation.

The unit exists because the search UI (note-search-ui ui-spec §2) converts `from:@acct` into an
author chip and offers candidates while the user types, whereas `searchPersonalNotes` only accepts a
canonical actor ID URL (`ActorUrl`). It records the user's decisions `d04-q13-author-suggestion-scope`,
`d04-q14-acct-resolution-basis` and `d04-q15-host-omitted-acct`.

## API inventory

Both routes are `GET`, use `bearer_auth` (a future contract: the caller must be allowed to act as the
Profile in the path) and live under `/api/v1/profiles/{profile_nanoid}`, where `profile_nanoid` is the
**viewing** Profile. Both belong to the OpenAPI tag `note-search-author`.

| Suffix | operationId | Query | Answer |
| --- | --- | --- | --- |
| /search/authors/resolve | resolveNoteSearchAuthor | `acct` (required, exactly one) | `ResolveAuthorResponse` |
| /search/authors/suggestions | suggestNoteSearchAuthors | `prefix` (required, exactly one) | `SuggestAuthorsResponse` |

Every future 200 and error carries `Cache-Control: private, no-store` (the answer depends on the
viewing Profile). The 501 stub does not set it. Unknown query parameters are ignored.

## Target set and what is never done

- **Target set.** Only authors ShuttlePub has **already fetched notes of** AND that the viewing Profile
  may **publicly refer to**. An author who is not fetched, or not referable by this Profile, does not
  exist for these operations.
- **Mute and block.** *Suggestions* never contain an author the viewing Profile mutes or blocks, whether
  the author individually or the author's domain. *Resolve* does not apply this exclusion: the acct was
  typed explicitly and the Profile itself set the mute/block, so nothing is leaked. What happens next
  stays the rule of `searchPersonalNotes` (#23): searching for an author of a muted/blocked domain is an
  ordinary empty `200`.
- **No shared display cache.** Emumet's shared display cache of remote actors is not a source. It also
  holds actors fetched because of **other** Profiles' block/mute, so using it would reveal that history.
- **Resolution basis.** An acct resolves only through (a) a username this server has **confirmed** for
  itself, or (b) a **verified** remote acct mapping. An acct derived for display
  (`preferred_username` + the actor's host, not verified by WebFinger) never resolves, even though
  suggestions may show it.
- **No external lookup.** The server never queries a remote server for these operations: no WebFinger,
  no actor fetch, neither synchronously nor asynchronously (and it does not enqueue one either). An acct
  it cannot confirm locally is simply not found. "Lookup with an external query" would be a future
  specification change, not an implementation detail.
- **No host guessing.** A host-less `@alice` is this server's `alice` (d04-q15). Nothing else is ever
  inferred; a host equal to this server's own host means the same as omitting it.

## resolveNoteSearchAuthor

Input `acct`, exactly one value; the pure check is `normalize_acct` and `parse_resolve_query`.
Accepted forms: `user@host`, `@user@host`, and without a host `user` / `@user`.

- Exactly **one** leading `@` is removed. The username is kept verbatim: no lowercasing, no Unicode
  normalisation. The host is normalised to lowercase ASCII (UTS #46 / IDNA A-label).
- A host must be a DNS name: at most 253 octets, labels of 1..=63 octets made of `a-z 0-9 -` without a
  leading or trailing hyphen. IP addresses (also numeric shorthand like `1.2.3` or `0x7f.1`), an empty
  label, a trailing dot, an underscore and a port are invalid. Single-label hosts such as `localhost`
  pass the syntax check.
- At most 512 characters after normalisation (`user` or `user@host`), and at most 513 raw characters
  (the optional `@` plus 512); the schema bound is 513, the 512 rule is checked on the normalised value.
- `400 invalid_request` with `details.field = acct` for: missing, empty or repeated `acct`; whitespace;
  control characters; Unicode bidi controls (U+061C, U+200E/F, U+202A–202E, U+2066–2069); U+FFFD (an
  invalid percent-encoded UTF-8 sequence); URL syntax (`/ \ ? # : < > [ ] %`, which is what rejects
  `from:` and `acct:` prefixes, actor URLs and ports); an empty username or host; more than one `@`
  separator; an invalid host; a too long value. The query is form-decoded (`+` is a space).

Answer `ResolveAuthorResponse { profile_id, author: SearchAuthor }`. Errors:

| Status | code | Meaning |
| --- | --- | --- |
| 400 | invalid_request | input rules above (`details.field = acct`) |
| 401 / 403 | unauthorized / forbidden | no credentials / cannot act as this Profile |
| 404 | not_found | the Profile, or an acct that is not a fetched, referable author this server can confirm (`details.field = acct`). Never produced by asking a remote server |
| 409 | conflict | several verified mappings match, so the acct cannot be decided (`details.field = acct`). The first match is never picked |
| 429 | rate_limited | `Retry-After` header (seconds) |
| 500 | internal | unexpected failure; never turned into a missing author |
| 501 | not_implemented | what the stub answers today |
| 503 | author_lookup_timeout / author_lookup_unavailable | the **internal** author lookup (not WebFinger) timed out or is unavailable; distinct from `search_timeout` of `searchPersonalNotes`; never turned into a missing author |

## suggestNoteSearchAuthors

Input `prefix`, exactly one value; the pure check is `normalize_prefix` and `parse_suggestions_query`.

- One leading `@` is removed; 1..=512 characters remain (513 raw). Spaces are allowed (display names
  contain them); control characters and U+FFFD are `400 invalid_request` (`details.field = prefix`),
  as are a missing, empty (also after removing the `@`), repeated or too long `prefix`.
- Matching is a **front match** of the prefix against the display name OR the acct. No substring, no
  regular expression, no wildcard; `%`, `_` and `\` are literal characters.
- Only for comparing, both sides are folded with `fold_for_match` (Unicode NFKC, then Unicode
  lowercasing). The text of a result is never changed: display name and acct are returned as stored.
- **Order**: (1) acct equals the prefix, (2) acct starts with the prefix, (3) display name starts with the
  prefix; within the same rank by normalised (folded) acct, then by actor URL (code point order). One
  entry per actor URL (the first in that order). At most **5** items; no paging, no total, no cursor.
- No candidate is an ordinary `200` with `items: []`; errors are never turned into an empty list.

Answer `SuggestAuthorsResponse { profile_id, items: SearchAuthor[0..5] }`. Errors: 400 (`prefix`), 401,
403, 404 (the Profile), 429 `rate_limited` with `Retry-After`, 500, 501 and 503
`author_lookup_timeout` / `author_lookup_unavailable` as above.

## Result types (closed)

`SearchAuthor { actor_url, acct, display_name, icon_url }`, all four keys required:

- `actor_url` is `ActorUrl`, the same type and constraints as the `author` of `searchPersonalNotes`: a
  successful result can be passed straight to that search (checked by the tests with its validator).
- `acct` is `user@host` without a leading `@` (1..=512 characters). An author of this server carries
  this server's own host. A resolve answer carries the confirmed/verified acct; a suggestion may carry an
  acct shown only for display, so a chosen candidate is identified by `actor_url`, not by resolving the
  text again.
- `display_name` is a string or `null`; `icon_url` is an `https` URL (at most 2048 characters) or `null`.

Unknown fields are rejected. Never returned: inbox, public key, Account ID, relationship state (follow,
mute, block), how the author was fetched, cache times. `SuggestAuthorsResponse.items` has at most 5
entries and no actor URL twice (the type rejects both; the schema states the maximum).

## Errors added to `ErrorCode`

`rate_limited` (429), `author_lookup_timeout` (503), `author_lookup_unavailable` (503). Existing codes
keep their meaning; `search_timeout` stays the note search time limit and is not reused. This is an
additive change of the generated `ErrorCode` enum, so frontends pinned to an earlier producer SHA must
re-pin before they can parse these codes.

## Deliberately out of scope (follow-up units)

- The real implementation: reading the set of authors ShuttlePub has fetched, a read-only integration
  that **never triggers a fetch** from Emumet, and storing and supplying verified remote acct mappings.
- Looking up with an external query (WebFinger / actor fetch): a possible future specification change.
- frontends: the generated schema pin and the screens. How a browser loads a remote `icon_url` (which
  reveals its IP address to that server unless proxied) belongs to the frontends' image policy.
- Rate limits and time limit values, caching of lookups, multi-author search.

## Evidence

`server/tests/note_search_author.rs` with `server/tests/fixtures/note_search_author/`:
pure input checks (accepted/rejected accts and prefixes, length boundaries, host normalisation, bidi and
control characters, URL syntax, `%`/`_` literals, Unicode names, query parsing); the generated
parameter, response and error schema of both operations; 58 positive/negative schema fixtures
cross-checked against the serde types; a test-only reference projection (37 suggestion cases and 21
resolve cases: ordering, de-duplication, the 5 item cut, mute/block exclusion, 409 for an ambiguous acct,
verified mappings only, the returned acct is the verified one (not the stored display acct), successful `actor_url` accepted by the #23 validator); both routes answering 501
for any input and credentials; and a check that no database, HTTP client or background dependency was
added. `tests/openapi.rs` keeps the route and schema snapshot and the checked-in schema equals the
generated one.
