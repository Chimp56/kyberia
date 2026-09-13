# Kyberia Lab MCP validation

The focused local gate is:

```sh
cd tools/lab-mcp
./node_modules/.bin/tsc -p tsconfig.json --noEmit
node --test --import tsx test/*.test.ts
node test/package-inventory.mjs
node test/supply-chain.mjs
PYTHONPYCACHEPREFIX=.trash/test-runs/pycache python3 -m unittest -v test/test_windows_process.py
```

Local results on 2026-09-13: TypeScript formatting and typecheck passed; all 30 Node tests completed with 29 passed, zero failed and one expected Windows-only skip. The suite covers the production-code stdio process chain, complete committed-tree materialization, checkout-dirt exclusion, executable SHA-256 pinning, persisted host-signature evidence, concurrent admission, timing bounds and noncooperating-process deadlines. The npm dry-run inventory contained only `dist/src` JavaScript plus npm's mandatory `package.json`/`README.md`; the deterministic CycloneDX/lock consistency check passed. Three fixed Windows catalog parser tests passed separately. Tests retain their run/replay/snapshot directories under `.trash/test-runs`. The same-host stdio test uses generated ephemeral credentials and a harmless fixed `/usr/bin/printenv KYBERIA_LAB_SEED` operation without claiming remote key separation or adding an arbitrary-shell surface.

The real-host procedure is in the package README. Current macOS cannot execute the Windows Job Object branch, CUDA, authorized Kismet radios or a true spectrum source. A current online npm advisory audit is also release-environment evidence because it requires registry access; the committed lock/SBOM drift gate remains local. These external gates are not reported as validated.
