# Original macOS protocol fixtures

All NDJSON files here are independently constructed synthetic inputs, not
captured CoreWLAN output. Every hello marks `evidence_origin=synthetic_fixture`.
The adapter-shaped `observed_api_result` payload is test data within that origin;
consumers must carry the stream origin through replay and must never call it
a real measurement. The fictional unicast BSSID and SSID `test-only` are original.

Regenerate with `python3 collectors/macos/fixtures/generate.py`. Tests check
deterministic bytes, unknown noise/width, denied/unsupported/error/partial states,
provenance and malicious mutations. No vendor source or fixture was copied.
License is NOASSERTION pending the project distribution/license decision.
Redistribution and new measured captures require their own source-ledger review.
