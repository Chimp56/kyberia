# Windows trusted-path fixture correction

Hosted run [34341800968](https://github.com/Chimp56/kyberia/actions/runs/34341800968), source c02212d, passed macOS and Ubuntu but failed Windows in `process::tests::trusted_path_rejects_relative_non_regular_and_non_executable_paths`. Public annotation 102434059305 supplies the test name; it does not provide a full panic trace.

Inspection found the fixture `/definitely/missing/collector` is not an absolute drive-qualified Windows path. Production correctly distinguishes relative paths from inaccessible absolute paths. The test now constructs an absolute missing child in a unique retained directory under `.trash/test-runs/`. It also checks directory rejection and, on Unix only, non-executable regular-file rejection. Production behavior is unchanged.

Independent reviewer Beauvoir approved the functional change with no BLOCKER or MAJOR findings. The retained-directory location nit was corrected to follow repository trash policy.

Validation: `cargo fmt --all`, `cargo test -p kyberia-observation-pipeline --locked --offline trusted_path_rejects_relative_non_regular_and_non_executable_paths` (1 passed), and `cargo clippy -p kyberia-observation-pipeline --all-targets --locked --offline -- -D warnings` passed on macOS. Native Windows confirmation remains open; local success does not close that gate.
