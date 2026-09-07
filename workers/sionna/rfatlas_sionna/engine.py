"""Private engine subprocess. No engine object may leave this module."""

import importlib.metadata
import hashlib
import json
import os
from pathlib import Path
import platform
import resource
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from rfatlas_sionna import AUDITED_REVISION, ENGINE_PINS, WORKER_VERSION
from rfatlas_sionna.contract import (MAX_REQUEST_BYTES, canonical_bytes, decode,
                                     digest, validate)


def execute(request):
    versions = {name: importlib.metadata.version(name) for name in ENGINE_PINS}
    if versions != ENGINE_PINS:
        raise RuntimeError("engine_version_mismatch")
    source_pin = json.loads(Path(__file__).with_name("source_pin.json").read_text())
    package_root = Path(importlib.metadata.distribution("sionna-rt").locate_file("sionna/rt"))
    for relative, expected in source_pin["python_files"].items():
        source = package_root / relative
        if not source.is_file() or hashlib.sha256(source.read_bytes()).hexdigest() != expected:
            raise RuntimeError("audited_source_mismatch: " + relative)
    import mitsuba as mi
    import drjit as dr
    # Select before importing sionna.rt: it binds native types on import.
    variants = mi.variants()
    mi.set_variant("llvm_ad_mono_polarized")
    import sionna.rt as rt
    versions.update({"python": platform.python_version(), "os": platform.platform(),
                     "machine": platform.machine(), "worker": WORKER_VERSION,
                     "backend": mi.variant(), "audited_source_revision": AUDITED_REVISION})
    versions["audited_python_source_verified"] = True
    versions["source_pin_sha256"] = digest(source_pin)
    llvm_path = os.environ.get("DRJIT_LIBLLVM_PATH")
    if not llvm_path or not Path(llvm_path).is_file():
        raise RuntimeError("LLVM native-build provenance requires DRJIT_LIBLLVM_PATH")
    versions["llvm_library_sha256"] = hashlib.sha256(Path(llvm_path).read_bytes()).hexdigest()
    capabilities = {"cpu_llvm": bool(dr.has_backend(dr.JitBackend.LLVM)),
                    "cuda": bool(dr.has_backend(dr.JitBackend.CUDA)),
                    "compiled_variants": variants,
                    "operations": ["validate_scene", "path_query", "radio_map"],
                    "scene_kinds": ["empty_space"], "product_tier_supported": False}
    if request["operation"] == "capabilities":
        return {"versions": versions, "capabilities": capabilities}
    started = time.monotonic()
    scene = rt.load_scene()  # Original empty scene; no upstream scene assets.
    scene.frequency = request["frequency_hz"]
    scene.bandwidth = request["bandwidth_hz"]
    scene.temperature = request["temperature_k"]
    scene.tx_array = rt.PlanarArray(num_rows=1, num_cols=1, pattern="iso", polarization="V")
    scene.rx_array = rt.PlanarArray(num_rows=1, num_cols=1, pattern="iso", polarization="V")
    for tx in request["transmitters"]:
        scene.add(rt.Transmitter(name=tx["id"], position=tx["position_m"]))
    for rx in request["receivers"]:
        scene.add(rt.Receiver(name=rx["id"], position=rx["position_m"]))
    settings = request["solver"]
    common = {**settings["interactions"], "max_depth": settings["max_depth"],
              "seed": settings["seed"]}
    data = {"kind": "scene_diagnostics", "valid": True,
            "geometry": "empty_space", "geometry_object_count": 0}
    if request["operation"] == "path_query":
        import numpy as np
        solver = rt.PathSolver()
        solver.loop_mode = settings["loop_mode"]
        paths = solver(scene, samples_per_src=settings["samples"],
                       max_num_paths_per_src=64, synthetic_array=True, **common)
        # CIR returns ordinary NumPy arrays; preserve path axis and absolute delays.
        a, tau = paths.cir(normalize_delays=False, out_type="numpy")
        a = np.asarray(a)
        tau = np.asarray(tau)
        power = np.sum(np.abs(a)**2, axis=(-2, -1))
        if not np.all(np.isfinite(power)) or not np.all(np.isfinite(tau)):
            raise RuntimeError("nonfinite_engine_result")
        data = {"kind": "point_paths", "units": {"path_gain": "linear_power_ratio",
                "delay": "s", "coefficient": "complex_amplitude_ratio"},
                "transmitter_ids": [x["id"] for x in request["transmitters"]],
                "receiver_ids": [x["id"] for x in request["receivers"]],
                "coefficient_shape": list(a.shape), "delay_shape": list(tau.shape),
                "path_gain_shape": list(power.shape), "path_gain": power.tolist(),
                "coefficients_real": a.real.tolist(), "coefficients_imag": a.imag.tolist(),
                "delays_s": tau.tolist(), "no_data_mask": (power == 0).tolist(),
                "combination": "sum_squared_path_amplitudes", "precision": str(a.dtype)}
    elif request["operation"] == "radio_map":
        import numpy as np
        grid = request["grid"]
        solver = rt.RadioMapSolver()
        solver.loop_mode = settings["loop_mode"]
        radio_map = solver(scene, center=grid["center_m"], orientation=[0, 0, 0],
                           size=grid["size_m"], cell_size=grid["cell_size_m"],
                           samples_per_tx=settings["samples"], rr_depth=-1,
                           stop_threshold=None, **common)
        gain = np.asarray(radio_map.path_gain)
        centers = np.asarray(radio_map.cell_centers)
        if not np.all(np.isfinite(gain)) or np.any(gain < 0):
            raise RuntimeError("nonfinite_engine_result")
        data = {"kind": "planar_radio_map", "units": "linear_power_ratio",
                "transmitter_ids": [x["id"] for x in request["transmitters"]],
                "shape": list(gain.shape), "axis_order": ["transmitter", "y", "x"],
                "path_gain": gain.tolist(), "cell_centers_m": centers.tolist(),
                "no_data_mask": (gain == 0).tolist(), "precision": str(gain.dtype),
                "grid": grid, "orientation_rad": [0, 0, 0],
                "combination": "monte_carlo_cell_average_power"}
    return {"versions": versions, "capabilities": capabilities, "data": data,
            "data_sha256": digest(data), "scene_sha256": request["scene_sha256"],
            "profile_revision": request["profile_revision"],
            "solver": settings, "frequency_hz": request["frequency_hz"],
            "bandwidth_hz": request["bandwidth_hz"], "temperature_k": request["temperature_k"],
            "noise_model": "not_used_for_path_gain", "elapsed_engine_s": time.monotonic()-started,
            "warnings": ["Empty-space LOS research proof; P2 product support is not established.",
                         "No geometry/material or field uncertainty has been quantified.",
                         "Zero sampled power is marked no-data; it is not measured absence.",
                         "Final Wi-Fi SINR, airtime and capacity require RF Atlas composition."]}


def main():
    # Reserve a protocol FD; quarantine Python and native stdout as bounded logs.
    protocol = os.fdopen(os.dup(sys.stdout.fileno()), "wb", buffering=0)
    os.dup2(sys.stderr.fileno(), sys.stdout.fileno())
    result = {"schema_version": 1, "status": "failed", "error": "invalid_request"}
    try:
        request = validate(decode(sys.stdin.buffer.read(MAX_REQUEST_BYTES + 1)))
        cpu_s = request.get("limits", {}).get("cpu_s", 30)
        resource.setrlimit(resource.RLIMIT_CPU, (cpu_s, cpu_s + 1))
        result.update({"request_id": request["request_id"], "request_sha256": digest(request)})
        result.update(execute(request))
        result["status"] = "completed"
        result.pop("error", None)
    except importlib.metadata.PackageNotFoundError:
        result["error"] = "engine_unavailable"
    except (ImportError, ModuleNotFoundError) as error:
        print(type(error).__name__ + ": " + str(error), file=sys.stderr)
        result["error"] = "backend_unavailable"
    except Exception as error:
        print(type(error).__name__ + ": " + str(error), file=sys.stderr)
        result["error"] = "engine_failure"
    protocol.write(canonical_bytes(result))
    protocol.close()


if __name__ == "__main__":
    main()
