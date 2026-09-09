# Canonical renderer correction review

Reviewer: root, independent of author Laplace. Frozen source:
`4cd60dc17ceba61db050f3c2208e39dcd054fb36`. Disposition: REQUEST_CHANGES.

## MAJOR — canonical byte admission still diverges

The original fixture loads successfully in both runtimes. Changing only the
schema string representation from `"V1"` to `"\u00561"`, or encoding the
integer grid width with a `.0` suffix, also returns `READY` from browser
`loadCanonicalScene`. Actual Rust `SceneDocument::from_canonical_bytes` rejects
the exact same byte files: `NonCanonicalBytes` and `MalformedBytes`, respectively.
Key order, duplicate-key and whitespace checks are insufficient to enforce
this canonical contract. The candidate must not be integrated as validated
canonical admission. The author is correcting this using the existing Rust
validator through an outward WebAssembly boundary.

| Input | Browser module | Native canonical adapter |
| --- | --- | --- |
| Original fixture | READY | Accepted |
| Escaped schema string | READY | NonCanonicalBytes |
| Float-encoded integer width | READY | MalformedBytes |

The browser module probe ran directly under Node, not a rendered browser session.
Browser plugin was unavailable; actual UI retesting will use pinned Playwright
after admission correction. This packet makes no new visual acceptance claim.

Exact input artifacts are retained under
`/private/tmp/kyberia-renderer-root-review-4cd60dc1`; native harness and output
are under `.trash/renderer-admission-proof-4cd60dc`.

| Input | SHA-256 |
| --- | --- |
| original | `24c2765339aceef830d05e57999e3b435a10092d2782ed9c2153e2c1e306fc8c` |
| escaped_schema | `f2c149fa91a04c766f2d460259b4dc8abe9887308ef021825e3d7197305d5206` |
| float_width | `dc863578f9774862f4652462ecb3811be8f451bebf8586f1930bc76db2660485` |
