# Neutral planning interchange draft review

Source: `411d1955eb00dfc913d9ba229272d94590d8da36` in the isolated
`planning-interchange` worktree. Reviewer: root, independent of author Russell.
Initial disposition: REQUEST_CHANGES; broader independent review remains open.

## MAJOR — schema version type confusion

Using the original fixture, replace only `schema.version` with Python `True`
or `1.0`. Both `encode_document` and `decode_canonical_document` accept the
result. `_validate_document` compares the version to integer 1 without checking
its type, allowing Python equality coercion to select the protocol version.
In particular, the Boolean wire value is not the declared numeric version.

The author must enforce the protocol's exact version type and test both direct
encoding and hostile wire decoding. Non-integer representations must not select
a version through coercion. This is independent of whether an unknown version
number is otherwise rejected.

Reproduction uses `runpy.run_path("research/interchange/interchange.py")`, reads
`research/interchange/fixtures/openrfplan-v1.json` with `json.loads`, deep-copies
it, changes `document["schema"]["version"]`, and calls
`decode_canonical_document(encode_document(document))`. Both mutations completed
successfully under the frozen source. No external runtime or data was used.

The frame contract also needs an unambiguous rotation convention for its angle
triple before claiming neutral geometric interpretation. Full security, bounds,
reference and deterministic round-trip review is still pending. This packet
does not approve integration or make upstream acceptance a release dependency.

## Correction checkpoint

Author correction `a0a18b3263ee790b43deaf68d00ed7d19f9b6591` adds exact
integer version admission and documents frame rotation conventions. Root reran
`python -m unittest discover -s tests -p test_planning_interchange.py -v`
in the isolated tree using the integration Python environment: all ten tests
passed. This includes direct-object and wire-level version rejection. A fresh
independent review of the complete proof remains required before integration;
this focused result does not close the broader review.
