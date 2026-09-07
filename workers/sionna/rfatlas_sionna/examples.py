"""Original unit-labelled acceptance scene; not an engine result fixture."""

from copy import deepcopy
from .contract import FRAME, INTERACTIONS, digest


def request(operation="path_query", seed=42, frequency_hz=2.4e9):
    scene = {"revision": "empty-space-v1", "kind": "empty_space", "coordinate_frame": FRAME,
             "bounds_m": [[-20, -20, 0], [20, 20, 10]]}
    return {"schema_version": 1, "request_id": "empty-space-proof", "operation": operation,
            "scene": scene, "scene_sha256": digest(scene), "profile_revision": "iso-v1",
            "frequency_hz": frequency_hz, "bandwidth_hz": 20e6, "temperature_k": 290,
            "transmitters": [{"id": "tx0", "position_m": [0, 0, 4]}],
            "receivers": [{"id": "rx0", "position_m": [10, 0, 4]}] if operation != "radio_map" else [],
            "grid": {"center_m": [0, 0, 1.5], "size_m": [8, 8], "cell_size_m": [2, 2]}
                    if operation == "radio_map" else None,
            "solver": {"backend": "llvm_ad_mono_polarized", "seed": seed, "samples": 100000,
                       "max_depth": 0, "interactions": deepcopy(INTERACTIONS),
                       "synthetic_array": True, "loop_mode": "evaluated",
                       "antenna": "isotropic-single-V"},
            "limits": {"timeout_s": 120, "cpu_s": 120}}
