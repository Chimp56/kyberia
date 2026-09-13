# Kyberia Lab MCP

Kyberia Lab is a development-only, stdio-only MCP server for repeatable validation on pinned lab hosts. It is not a product data plane and its responses are evidence, not canonical Kyberia domain objects. There is no HTTP listener, arbitrary shell tool, request-supplied executable, path, URL, environment, or argument vector.

The coordinator exposes `lab://hosts`, `lab://hosts/{id}/capabilities`, `lab://runs/{id}`, `lab://runs/{id}/manifest`, and `lab://runs/{id}/artifacts`. Its ten tools are the exact requested validation, status, cancellation, probe, gate and sanitized-artifact operations. A full lowercase 40-hex Git object name must also appear in the operator's immutable revision list.

## Bootstrap and checks

From this directory, using the repository-pinned Node and pnpm binaries:

```sh
node /absolute/path/to/pnpm.mjs install --frozen-lockfile --store-dir .tools/pnpm-store
./node_modules/.bin/tsc -p tsconfig.json
node --test --import tsx test/*.test.ts
PYTHONPYCACHEPREFIX=.trash/test-runs/pycache python3 -m unittest -v test/test_windows_process.py
```

Generate separate Ed25519 key pairs with an approved secret-management tool. Store DER PKCS#8 private keys and DER SPKI public keys as base64 environment secrets. The coordinator has its own private key and each remote runner service has only that coordinator's public key. Each runner's private key stays in its host-side secret service; the coordinator has only the pinned host public key. A remote fixed transport may forward no credentials when its service resolves runner credentials locally. `credentialEnvNames` is an explicit allowlist, not a required-key list.

The direct-local development mode and stdio E2E run coordinator and runner under the same OS account, so their generated ephemeral keys co-reside and the local operation explicitly forwards the runner private key. Do not use that arrangement for a remote host. Rotate production keys by adding a new key ID/config, draining old jobs, then removing the old secret. Never commit key material.

Copy and edit `config.example.json` and `runner.example.json`. Every coordinator operation points to the fixed runner entry point and names the only signing-key environment variables that may cross that process boundary. The runner independently maps the signed suite and bounded parameters to its administrator-owned static command table. It verifies the coordinator signature, key ID, freshness, nonce uniqueness, exact checked-out revision, operation version, input-manifest identity and timeout before execution. Every manifest entry must be a unique, ordinary, tracked file whose bytes match the operator-pinned SHA-256; executable or script files inside the checkout must be included. The runner passes the seed only through `KYBERIA_LAB_SEED`, together with fixed spec-version and input-manifest environment values. It accepts no request-supplied argument.

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
