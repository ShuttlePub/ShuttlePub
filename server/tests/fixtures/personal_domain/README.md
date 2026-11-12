# S8 contract fixtures

These are future-contract examples, not responses from a working backend.
All actual /api/v1 routes return 501. Test-only reference projections check the
semantics; production does not filter or enforce policies.

- e8_contract.json: pinned upstream schema evidence and E8/D8P observations, not a local normalizer.
- schema_cases.json: schema-valid and schema-invalid payloads, including arbitrary hidden extensions.
- surfaces.json: mute/block, another Profile, whole quotes/boosts, canonical-host/exact-domain matching, unread counts.
- reveal.json: initial hidden, single-note display, other-note isolation, hide/reload/revisit, quoted-target isolation, independent gates.
- rejections.json: five distinct actions in each direction; D13 allow does not bypass block.
- counts.json: accepted historical counts survive block and refused new attempts.

See ../../../docs/personal-domain-content-contract.md for scope and E8 correspondence.
