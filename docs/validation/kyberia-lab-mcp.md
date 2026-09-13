# Kyberia Lab MCP validation

The focused local gate is:

```sh
cd tools/lab-mcp
./node_modules/.bin/tsc -p tsconfig.json --noEmit
node --test --import tsx test/*.test.ts
node test/package-inventory.mjs
PYTHONPYCACHEPREFIX=.trash/test-runs/pycache python3 -m unittest -v test/test_windows_process.py
```

Local results on 2026-09-13: TypeScript build passed; 24 Node tests passed and one Windows-only fail-closed assertion was skipped, including the production-code stdio process chain and two noncooperating-process deadline cases; the npm dry-run inventory passed; three fixed Windows catalog parser tests passed. Tests retain their run/replay directories under `.trash/test-runs`. The same-host stdio test uses generated ephemeral credentials and a harmless fixed `/usr/bin/printenv KYBERIA_LAB_SEED` operation. It proves coordinator credential allowlisting, runner authentication, signed seed binding, immutable tracked input binding and signed evidence publication without claiming remote key separation or adding an arbitrary-shell surface.

The real-host procedure is in the package README. Current macOS cannot execute the Windows Job Object branch, CUDA, authorized Kismet radios or a true spectrum source. These hardware/OS gates do not invalidate the strict protocol and local contract evidence and are not reported as validated.
