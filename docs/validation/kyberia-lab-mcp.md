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

Local results on 2026-09-13: TypeScript formatting and typecheck passed; all 33 Node tests completed with 32 passed, zero failed and one expected Windows-only skip. The suite covers the production-code stdio process chain, closed direct-or-single-bundle runner invocation and argument-bypass tables, complete committed-tree materialization, checkout-dirt exclusion, executable/bundle SHA-256 pinning and tamper rejection, commit-versus-tree verification, coordinator/host key-role separation, offline verification of the persisted signed request, safe host payload, every job-identity mapping and artifact hashes, concurrent admission, timing bounds and noncooperating-process deadlines. The npm dry-run inventory contained only `dist/src` JavaScript, the self-contained runner bundle, dependency SBOM, npm's mandatory `package.json`/`README.md`, the three documented configuration schematics, and the vendored CycloneDX validation schemas/license. The structural pnpm-lock/CycloneDX consistency check validates known direct/transitive coordinates, registry integrity hashes, license identifiers and the official CycloneDX 1.6 schema. Three fixed Windows catalog parser tests passed separately. Tests retain their run/replay/snapshot directories under `.trash/test-runs`. The same-host stdio test uses generated ephemeral credentials and a harmless fixed `/usr/bin/printenv KYBERIA_LAB_SEED` operation without claiming remote key separation or adding an arbitrary-shell surface.

The real-host procedure is in the package README. A network-authorized
`pnpm audit --prod --json` on 2026-09-13 reported zero vulnerabilities across
92 production dependencies. Current macOS cannot execute the Windows Job Object
branch, authorized Kismet radios or a true spectrum source. These
hardware/host gates are not reported as validated.
