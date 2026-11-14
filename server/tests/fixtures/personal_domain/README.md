# S8 contract fixtures

These are future-contract examples, not responses from a working backend.
All actual /api/v1 routes return 501. Test-only reference projections check the
semantics; production does not filter or enforce policies.

- e8_contract.json: pinned upstream schema evidence and E8/D8P observations, not a local normalizer.
- schema_cases.json: schema-valid and schema-invalid payloads, including arbitrary hidden extensions.
- surfaces.json: mute/block, another Profile, whole quotes/boosts, canonical-host/exact-domain matching, unread counts.
- reveal.json: initial hidden (`cause: direct`), single-note display, other-note isolation, hide/reload/revisit, independent gates.
- hidden_quote.json: D08 ui-spec 3-4. A quote of a controlled domain's note is hidden with `cause: quote` naming the quoted source's domain, and one reveal shows the quote together with its source (CW/sensitive targets stay restricted; a deleted/unviewable target never yields `cause: quote`); direct vs quote vs both (including the author and the quoted source under two different controls of one Profile), other Profile, exact host match, and that timeline omission stays whole-quote.
- rejections.json: five distinct actions in each direction; D13 allow does not bypass block.
- counts.json: accepted historical counts survive block and refused new attempts.
- remote_emoji_chips.json: D09 reaction chips with external custom emoji (`EmojiReactionCount.view.origin`), same-named emoji of different servers, and chips that violate the per-chip rules.

See ../../../docs/personal-domain-content-contract.md for scope and E8 correspondence.
