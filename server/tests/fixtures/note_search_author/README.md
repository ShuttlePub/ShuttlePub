# Note search author lookup fixtures

Future-contract examples for `resolveNoteSearchAuthor` and `suggestNoteSearchAuthors`, not responses
of a working backend: every route answers 501 and nothing here implies an external lookup.

- schema_cases.json: schema-valid and schema-invalid payloads (local/remote authors, null display fields,
  0/1/5 suggestions, 6 suggestions, bad actor_url, missing keys, unknown fields such as inbox, and the
  400/404/409/429/501/503 error bodies). `serde_rejects` records whether the Rust type refuses the payload too.
- projection.json: data of the TEST-ONLY reference projection: fetched / publicly referable authors, per
  viewer mute/block state, verified acct mappings, and the suggestion and resolve cases it must satisfy.

See ../../../docs/note-search-author-lookup-contract.md for scope and rules.
