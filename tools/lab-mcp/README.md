# Kyberia Lab MCP

Kyberia Lab is a development-only, stdio-only MCP server for repeatable validation on pinned lab hosts. It is not a product data plane and its responses are evidence, not canonical Kyberia domain objects. There is no HTTP listener, arbitrary shell tool, request-supplied executable, path, URL, environment, or argument vector.

The coordinator exposes `lab://hosts`, `lab://hosts/{id}/capabilities`, `lab://runs/{id}`, `lab://runs/{id}/manifest`, and `lab://runs/{id}/artifacts`. Its ten tools are the exact requested validation, status, cancellation, probe, gate and sanitized-artifact operations. A full lowercase 40-hex Git object name must also appear in the operator's immutable revision list.

## Bootstrap and checks

From this directory, using the repository-pinned Node and pnpm binaries:

```sh
node /absolute/path/to/pnpm.mjs install --frozen-lockfile --store-dir .tools/pnpm-store
./node_modules/.bin/tsc -p tsconfig.json
node --test --import tsx test/*.test.ts
node test/supply-chain.mjs
PYTHONPYCACHEPREFIX=.trash/test-runs/pycache python3 -m unittest -v test/test_windows_process.py
```

Generate separate Ed25519 key pairs with an approved secret-management tool. Store DER PKCS#8 private keys and DER SPKI public keys as base64 environment secrets. The coordinator has its own private key and each remote runner service has only that coordinator's public key. Each runner's private key stays in its host-side secret service; the coordinator has only the pinned host public key. A remote fixed transport may forward no credentials when its service resolves runner credentials locally. `credentialEnvNames` is an explicit allowlist, not a required-key list.

The direct-local development mode and stdio E2E run coordinator and runner under the same OS account, so their generated ephemeral keys co-reside and the local operation explicitly forwards the runner private key. Do not use that arrangement for a remote host. Rotate production keys by adding a new key ID/config, draining old jobs, then removing the old secret. Never commit key material.

The packaged `config.example.json`, `runner.example.json`, and `config.remote.example.json` are schemas with explicit zero/hash/path replacement markers; they are deliberately non-provisioned and cannot authenticate until an operator replaces every marker. The tested runnable contract is `test/stdio.test.ts`: it generates distinct ephemeral keys, a complete exact-commit manifest, absolute pinned runtime/bundle/Git/operation identities, and a retained same-host config before exercising a real MCP client. The local coordinator fixture deliberately forwards the local host key because both processes share one development account. The remote schematic uses an authenticated fixed transport with an empty credential allowlist so the host private key stays in the remote runner service. Configuration and runtime fingerprint checks forbid sharing coordinator and host key material.

Every coordinator operation uses a closed invocation union. A `direct` invocation executes one absolute SHA-256-pinned native transport with no arguments. A `single-pinned-bundle` invocation executes one absolute SHA-256-pinned runtime with exactly one absolute SHA-256-pinned self-contained runner bundle. Both forms reject every caller-configured argument, inline command, loader, import, module selector, shell flag, and relative entry point before any credential is forwarded. The runner independently maps the signed suite and bounded parameters to its administrator-owned static command table, whose Git and operation executables are also absolute and SHA-256 pinned. Generate the complete tree manifest after building with:

```sh
npm run input-manifest -- /usr/bin/git <git-executable-sha256> /absolute/kyberia <40-hex-commit>
```

Paste its ID and entries into runner configuration. The generator and runner reject symlinks, submodules, missing/duplicate entries and trees beyond fixed file/byte bounds. The runner reads every blob from the exact commit, verifies the complete manifest, then exclusively creates a retained `.trash` snapshot and executes with that snapshot as the working directory. Dirty, untracked and ignored checkout files are therefore outside the command namespace. Replay claims and snapshots cannot be reused and are intentionally retained for audit.

The runner verifies the coordinator signature, key ID, freshness, nonce uniqueness, operation version, input-manifest identity and timeout before execution. The requested object must be a commit rather than a tree. Commands receive an empty `PATH`; snapshot scripts cannot reach ignored `.tools` by relative lookup. The runner passes the seed only through `KYBERIA_LAB_SEED`, together with fixed spec-version and input-manifest environment values. It accepts no request-supplied argument. The host sanitizes bounded text before signing. Persisted evidence contains the complete safe signed job request, complete safe host payload, signed digests, logical tool IDs/versions/digests, and enough duplicated fields to verify every job identity and published artifact hash offline without exposing host paths. Coordinator-only terminal outcomes use an explicit evidence-origin variant.

Unix descendants are held in a process group and face a hard deadline after cooperative termination. Run intent and terminal state are atomically replaced on disk; a restart converts interrupted work into a signed failed recovery manifest. The retained fixed-catalog Windows helper uses Kyberia's tested Job Object primitive, but the TypeScript runner fails closed on Windows until the helper is integrated and exercised on a real Windows host.

Run the coordinator:

```sh
KYBERIA_LAB_CONFIG=/absolute/coordinator.json node dist/src/main.js
```

For Codex, add a local stdio server entry to the client configuration after building:

```toml
[mcp_servers.kyberia-lab]
command = "/absolute/node"
args = ["/absolute/kyberia/tools/lab-mcp/dist/src/main.js"]
env = { KYBERIA_LAB_CONFIG = "/absolute/coordinator.json" }
```

Keep signing keys in the launching service's secret environment rather than the TOML file. Restart Codex after changing MCP client configuration. In Docker MCP Toolkit, register the same stdio command as a custom local server, mount only the built package, read-only coordinator config and `.trash/lab-runs`, and inject keys through Docker secrets. Do not expose it through an unauthenticated HTTP transport.

## Runtime validation

1. Pin a clean checkout at an admitted commit and provision coordinator/host keys separately.
2. Start the runner through the coordinator's fixed executable specification.
3. Use a real MCP client to list tools, resources and resource templates.
4. Run `probe_wifi_capabilities`, then a foundation validation with an explicit seed and timeout.
5. Read run status, signed manifest and artifact inventory; independently verify both host response and coordinator manifest signatures and every artifact SHA-256.
6. Repeat a request to prove nonce replay rejection; tamper with host, key, revision and artifact bytes to prove failure.
7. Cancel queued and running jobs and verify bounded descendant termination plus signed cancelled manifests.
8. On authorized Linux/Kismet, CUDA and spectrum hosts, run their named gates. On Windows, run the Job Object descendant fixture and retain hosted evidence.

Only UTF-8 text summaries and logs use artifact class `kyberia-lab-text-v2`. The sanitizer bounds its working set and removes colon/hyphen MAC addresses, IPv4/IPv6 addresses, SSID values, local paths, common JSON/plain secret fields, cloud key IDs and Basic/Bearer credentials. Raw PCAP, PCAPNG, KismetDB, spectrum IQ, location traces and arbitrary files are never returned by this server.

`pnpm-lock.yaml` pins the complete npm graph with registry integrity hashes. `dependency-sbom.cdx.json` is a deterministic CycloneDX inventory generated from a structural YAML parse of that lock and installed package license metadata. `npm run check` binds it to the complete lock hash, checks known direct/transitive coordinates, validates purls/hashes/licenses, and validates the document against the packaged official CycloneDX 1.6, JSF, and SPDX schemas. `npm run sbom` refreshes it. Run `pnpm audit --prod` in a network-authorized release environment because vulnerability advisory freshness is an external online dependency.
