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

## Rust/WASM correction browser checkpoint

Root independently tested frozen correction
`1af1cba8695573b5cd13b4b04733c61005c27417`. The two canonical-byte
reproductions above now reject through the Rust WASM worker, including actual
file-input interactions. This closes the reproduced admission divergence;
full architecture/security review remains pending before integration.

Environment: macOS ARM64, pinned Playwright Chromium, local Vite at
`http://127.0.0.1:4173/index.html`, desktop 1280×900 and mobile 390×844 at
device scale factor 2. Browser plugin not available. The existing server's
working directory was verified with `lsof` before reuse. `pnpm` was unavailable
on PATH; `npm` executed the existing scripts without installing dependencies.

Commands: `npm test` passes 19 tests; `npm run test:browser-canonical --
http://127.0.0.1:4173/index.html /private/tmp/kyberia-renderer-root-1af1cba`
passes the desktop/mobile/custom/OpenLayers regression matrix. Root also ran
an independent temporary `root-check.mjs` for page identity, keyboard control,
invalid-state inspection, and warning capture.

| Check | Evidence |
| --- | --- |
| Page identity / nonblank | Expected URL and title; numeric inspector present |
| Framework overlay | No Vite error overlay |
| Interactions | Enter on focused zoom button changes zoom 1 → 1.35 |
| Invalid input | Duplicate key, escaped schema, float width reject; scene and last render clear |
| Numeric geometry | World bounds remain [0, 0, 8, 6] metres |
| Responsive | No horizontal page overflow at either viewport |
| Screenshots | Root inspected desktop, mobile and invalid-state captures |
| Console | No app errors; four GPU readback performance warnings retained |

The existing browser regression listens for errors but not warnings. The root
check initially failed its zero-warning assertion on `GPU stall due to
ReadPixels`, then reran with only that observed driver-warning class allowed
and retained. This is performance evidence to assess before choosing the
renderer, not evidence of a warning-free application. Captures and the exact
temporary script/result are retained under
`/private/tmp/kyberia-renderer-root-1af1cba`.

This remains a research harness. Production UX, complete accessibility,
large-project performance, another OS/browser, and Gate B selection are open.
