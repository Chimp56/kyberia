# Independent review: isolated Sionna CPU proof

Verdict: **APPROVED for the bounded Phase 0 empty-space CPU proof**. No unresolved BLOCKER or MAJOR findings remain. This is not approval of full Gate I, supported P2/P3, CUDA, materials or field accuracy.

Reviewer: Storage/Data specialist, independent of the Sionna author. Source was inspected in the author's frozen worktree; review output resides in a separate `review/sionna-cpu` worktree. The complete plan was read, with this review focused on §8.15, §16.13, Phase 0, PREB-010–013, OSS-004–006/011, Gate I and Appendix I boundaries.

## Findings and resolution

| Severity | ID | Finding | Resolution |
|---|---|---|---|
| MAJOR | SRT-R001 | A checksummed completed response accepted missing Python provenance or a non-string Python version; OS/machine/native-build fields were also not validated. | Required typed Python ≥3.10 version, bounded nonempty OS/machine text and a 64-hex LLVM library hash are now validated for completed results. The producer explicitly fails when the configured native library provenance is unavailable. Independent malformed/missing field probes now reject. |
| MAJOR | SRT-R002 | A positive-power LOS path accepted zero delay despite request separation ≥1 m. Negative-only checks were insufficient. | Delay must be positive and agree with distance/c for this explicitly restricted LOS geometry, within 1e-5 relative or 2 mm/c absolute tolerance for float32 coordinates. Independent zero, tiny-positive and wrong-positive delay mutations now reject even after recalculating the data hash. |

There are no unresolved MINOR/NIT findings affecting acceptance. The review did not expand the declared empty-space proof into late-phase material or geometry requirements.

## Independently executed checks

- `python3 -m unittest discover -s tests -p test_sionna_worker.py -v`, author worktree, corrected frozen source: **17 passed in 1.123 s**. These are contract and actual process-lifecycle tests, not 17 full propagation runs.
- Eight independent checksummed result mutations: zero/tiny/wrong-positive delay; missing/malformed Python version; missing OS, machine and LLVM build hash: **all rejected** after correction.
- The audited archive SHA-256 matches the source ledger. All **49 Python source files** extracted in memory from that exact archive match `source_pin.json`; all 49 installed package files match that pin. No upstream code was copied into the review or core.
- Fresh corrected-source CPU jobs at 5.8 GHz: **six independent transmitter/receiver links passed** Friis power and distance/c checks. Transmitters `(0,0,4)` and `(4,3,4)`; receivers `(7,1,4)`, `(-2,8,4)`, `(12,-4,4)` meters. The asymmetric 3-receiver/2-transmitter matrix independently checks axis attribution. Maximum power relative error: **1.513993e-7**, against a 1e-5 tolerance.
- Fresh corrected-source CPU map: two transmitters above a **3×2** grid, center `(1,-1,1.5)` m, size `(6,4)` m, 2 m cells, 100,000 samples, seed 42. All 12 transmitter/cell values passed independent 80×80 midpoint integration of Friis power over each cell; maximum relative error **2.170995%**, below the stated 5% tolerance. Cell coordinates and `[transmitter,y,x]` mapping were checked separately.
- Independently reran the unmodified, pinned upstream CIR/LOS subset using `upstream_tests.py` with output directed into the review's ignored artifacts: **19 passed in 29.37 s**, process exit 0. This is a subset, not the complete upstream suite.
- Inspected the refreshed author CPU report: **34 checks PASS**, including its near-coordinate-limit delay case. This count is author runtime evidence and was not represented as an independent rerun of all 34 checks.

The actual interpreter was the isolated audited Python 3.12.12 environment, with Sionna RT 2.0.1 from commit `bc0549155c7b782c7614a0ec06a0ac4e32b979ae`, Mitsuba 3.8.0, Dr.Jit 1.3.1, LLVM 18.1.8, macOS ARM64 and `llvm_ad_mono_polarized`. Fresh jobs used `DRJIT_LIBLLVM_PATH=/opt/homebrew/opt/llvm@18/lib/libLLVM.dylib`; numerical output and full requests were retained under the review's ignored `.tools/sionna-review/` directory.

## Architecture, science and limits

The domain boundary returns unit-labeled engine-neutral JSON. Pure contract/client imports do not load Sionna. Trusted launcher arguments choose the interpreter; requests cannot select arbitrary programs or scene paths. Bounded JSON, process output/log limits, CPU/wall limits, cancellation, crash handling and explicit engine absence are implemented. No silent P0/P1 fallback or generic Sionna SINR enters Wi-Fi semantics. The scene is actually empty space; declared bounds do not masquerade as walls or floors. Map baselines integrate area power rather than comparing a cell average against center-point RSS.

The four-file PyPI/audited-source discrepancy is preserved, and the default proof requires the exact audited source. Locks, source and native-library hashes support reproducibility; packaging/license review and a release SBOM remain open. The reviewed code does not claim hard memory containment, an OS filesystem/network sandbox, arbitrary scene/material/antenna support, measured superiority, full upstream coverage or CUDA support. Runtime cancellation checks can interrupt initialization; sustained-kernel cancellation and OOM behavior remain part of broader Gate I validation.

An optional descendant-state probe encountered the sandbox's denial of `ps`; its escalation was aborted without a completed result. No descendant-state runtime claim is based on that probe. Existing process-group termination/reaping code was inspected, and the stated lifecycle tests above passed. This optional review check does not convert an unfinished product gate into an external blocker.

## Immutable reviewed files

The following hashes identify the corrected frozen implementation and evidence approved by this review. Integration must verify them before relying on this verdict.

| File | SHA-256 |
|---|---|
| `docs/adapters/sionna.md` | `49ab44e8b58f0ffd3c0d2f1a3e3098d69144db444eb6dcc0a823e75292bbd8f1` |
| `docs/licenses/sionna-sources.json` | `91d2922ae170c7909631854bbc580359792939d91206a956e5e3521794ab91c0` |
| `tests/test_sionna_worker.py` | `1a3ab59bc98aea01b3ec4fed731d775a2af22ea1b8a9b457d4fcc19cb2ac14a8` |
| `workers/sionna/.gitignore` | `a6b96789c16d886f5a0fac047442c7636aa17a973d382e94e692d897a4c7d111` |
| `workers/sionna/acceptance.py` | `88fbb173c580c31728414ac0e0000a64e6e8ff883ef6c959c6c5fb31d4b29b9c` |
| `workers/sionna/build-requirements.lock` | `83ddb11848a47496baad7d5cb6328f8e6097a0ac26c8d8c653c126de1f2b5c82` |
| `workers/sionna/build_wheel.py` | `70bff6b28e9a57250d09152eed6493eb0e18d3b44bf4c1efc52d3df1b7301f51` |
| `workers/sionna/evidence/cpu-proof.json` | `de63f7b1770e6ec4c6105d52861d4974734a8abfd7f343d114bd082d7ee41172` |
| `workers/sionna/evidence/initial-low-sample-failure.json` | `8e45d001bb628c83861e45c2894b108d928b8e5b35e6fbd699edf4f28accf18d` |
| `workers/sionna/evidence/upstream-subset.json` | `74e8d557e1c4c69b2e15f4ea0fc091441d38b242787d373b714bc37c7345dcb3` |
| `workers/sionna/inventory.py` | `d80aeb23cfa84f119cd35acb28321fe35b45c971f36cf1206441edd2b07f3bc0` |
| `workers/sionna/requirements.lock` | `c5aafe9464b7def46652f7579ce445766fcabc68b46999d3a37a3006486152fc` |
| `workers/sionna/rfatlas_sionna/__init__.py` | `4dfde659bf9f76bdcb75abb437f1a56615e2758a3a37d5331de282d353442fe2` |
| `workers/sionna/rfatlas_sionna/client.py` | `f437e11bd3afce9bb0695e970551ca7d162044c957eac28593fe0555fee76572` |
| `workers/sionna/rfatlas_sionna/contract.py` | `206c9ab110a2f3064d9b74ad5c97c19880622c95b5a48f55ca236783660682eb` |
| `workers/sionna/rfatlas_sionna/engine.py` | `6763114fef6b4b21395c1fac71af161ba305cb2622254a3a29544e74b6d20a09` |
| `workers/sionna/rfatlas_sionna/examples.py` | `d851e2ecb031348c0291417c79b397336a32cfcde93c093957fdc26dacfe6a98` |
| `workers/sionna/rfatlas_sionna/source_pin.json` | `1283b03d699d80693e969601602c21df3e89c427e086b4ac8ee1fa051273d564` |
| `workers/sionna/upstream_tests.py` | `2de9beece18853b435bfc35b68c3e3508237dd0dc392eeeae365aebebebc4126` |
| `workers/sionna/worker.py` | `296abe02d2e9509c52fff754127719ed23c8849dc6c522b19ceb378592d0013d` |
