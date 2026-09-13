# Kyberia Lab MCP validation

The focused local gate is:

```sh
cd tools/lab-mcp
./node_modules/.bin/tsc -p tsconfig.json --noEmit
node --test --import tsx test/*.test.ts
PYTHONPYCACHEPREFIX=.trash/test-runs/pycache python3 -m unittest -v test/test_windows_process.py
```

Local results on 2026-09-13: TypeScript passed; ten Node tests passed; three fixed Windows catalog parser tests passed. Tests retain their run/replay directories under `.trash/test-runs`. The official SDK `Client` exercises the real `McpServer` through the SDK's linked transport; a production stdio process/host-runner E2E remains a separate-host runtime gate because the coordinator must never possess the host-only private key.

The real-host procedure is in the package README. Current macOS cannot execute the Windows Job Object branch, CUDA, authorized Kismet radios or a true spectrum source. These hardware/OS gates do not invalidate the strict protocol and local contract evidence and are not reported as validated.
