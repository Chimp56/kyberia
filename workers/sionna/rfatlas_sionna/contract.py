"""Engine-neutral, deliberately small Phase 0 protocol; no arbitrary scene files."""

import hashlib
import json
import math
from pathlib import Path
import re

MAX_REQUEST_BYTES = 65536
MAX_RESULT_BYTES = 1048576
MAX_LOG_BYTES = 65536
FRAME = "building-local-right-handed-z-up-meters"
INTERACTIONS = {"los": True, "specular_reflection": False,
                "diffuse_reflection": False, "refraction": False,
                "diffraction": False, "edge_diffraction": False,
                "diffraction_lit_region": False}


class ContractError(ValueError):
    pass


def canonical_bytes(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"),
                      ensure_ascii=True, allow_nan=False).encode("ascii")


def digest(value):
    return hashlib.sha256(canonical_bytes(value)).hexdigest()


def _pairs(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ContractError("duplicate JSON field")
        result[key] = value
    return result


def decode(data, maximum=MAX_REQUEST_BYTES):
    if not isinstance(data, bytes) or len(data) > maximum:
        raise ContractError("message exceeds byte limit")
    try:
        return json.loads(data, object_pairs_hook=_pairs,
                          parse_constant=lambda _: (_ for _ in ()).throw(
                              ContractError("non-finite JSON number")))
    except (ValueError, UnicodeError, RecursionError) as error:
        raise ContractError("invalid bounded JSON message") from error


def fields(value, required):
    if type(value) is not dict or set(value) != set(required):
        raise ContractError("missing or unsupported fields: " + ",".join(required))


def number(value, low, high, integer=False):
    if type(value) not in ((int,) if integer else (int, float)):
        raise ContractError("expected finite numeric value")
    if not low <= value <= high:
        raise ContractError("numeric value outside supported range")
    return value


def vector(value, size, low=-10000, high=10000):
    if type(value) is not list or len(value) != size:
        raise ContractError("invalid vector shape")
    return [number(x, low, high) for x in value]


def identifier(value):
    if type(value) is not str or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,63}", value):
        raise ContractError("invalid bounded identifier")


def validate(request):
    if type(request) is not dict:
        raise ContractError("request must be an object")
    if request.get("operation") == "capabilities":
        fields(request, ("schema_version", "request_id", "operation"))
    else:
        fields(request, ("schema_version", "request_id", "operation", "scene",
                         "scene_sha256", "profile_revision", "frequency_hz",
                         "bandwidth_hz", "temperature_k", "transmitters", "receivers",
                         "grid", "solver", "limits"))
    if type(request["schema_version"]) is not int or request["schema_version"] != 1:
        raise ContractError("unsupported schema version")
    identifier(request["request_id"])
    if request["operation"] == "capabilities":
        return request
    if request["operation"] not in ("validate_scene", "path_query", "radio_map"):
        raise ContractError("unsupported operation")
    scene = request["scene"]
    fields(scene, ("revision", "kind", "coordinate_frame", "bounds_m"))
    identifier(scene["revision"])
    if scene["kind"] != "empty_space" or scene["coordinate_frame"] != FRAME:
        raise ContractError("only empty-space scenes in building meters are supported")
    bounds = scene["bounds_m"]
    if type(bounds) is not list or len(bounds) != 2:
        raise ContractError("invalid scene bounds")
    lo, hi = [vector(v, 3) for v in bounds]
    if any(b - a < 0.1 for a, b in zip(lo, hi)):
        raise ContractError("empty or reversed scene bounds")
    if request["scene_sha256"] != digest(scene):
        raise ContractError("scene checksum mismatch")
    identifier(request["profile_revision"])
    number(request["frequency_hz"], 1e9, 10e9)
    number(request["bandwidth_hz"], 1e6, 320e6)
    number(request["temperature_k"], 1, 1000)
    txs, rxs = request["transmitters"], request["receivers"]
    if type(txs) is not list or not 1 <= len(txs) <= 4:
        raise ContractError("one to four transmitters required")
    if type(rxs) is not list or len(rxs) > 16:
        raise ContractError("at most sixteen receivers supported")
    ids = set()
    for device in txs + rxs:
        fields(device, ("id", "position_m"))
        identifier(device["id"])
        if device["id"] in ids:
            raise ContractError("duplicate radio ID")
        ids.add(device["id"])
        position = vector(device["position_m"], 3)
        if not all(a <= x <= b for a, x, b in zip(lo, position, hi)):
            raise ContractError("radio outside declared scene extent")
    # Far-field-only proof with fixed isotropic co-polarized single-element arrays.
    for tx in txs:
        for rx in rxs:
            if math.dist(tx["position_m"], rx["position_m"]) < 1:
                raise ContractError("transmitter/receiver separation must be at least one meter")
    grid = request["grid"]
    if request["operation"] == "path_query":
        if not rxs or grid is not None:
            raise ContractError("path query needs receiver points and no grid")
    elif request["operation"] == "radio_map":
        if rxs:
            raise ContractError("radio maps use grid receivers only")
        fields(grid, ("center_m", "size_m", "cell_size_m"))
        center = vector(grid["center_m"], 3)
        size = vector(grid["size_m"], 2, 0.1, 1000)
        cell = vector(grid["cell_size_m"], 2, 0.1, 1000)
        counts = [s / c for s, c in zip(size, cell)]
        if any(abs(n - round(n)) > 1e-8 or n < 1 for n in counts):
            raise ContractError("grid dimensions must be exact cell-size multiples")
        if math.prod(round(n) for n in counts) > 4096:
            raise ContractError("radio map exceeds cell budget")
        if not lo[2] <= center[2] <= hi[2] or any(
                center[i] - size[i]/2 < lo[i] or center[i] + size[i]/2 > hi[i]
                for i in (0, 1)):
            raise ContractError("grid outside scene extent")
        if any(abs(tx["position_m"][2] - center[2]) < 1 for tx in txs):
            raise ContractError("radio map plane must be at least one meter below/above transmitters")
    elif grid is not None:
        raise ContractError("scene validation does not accept an execution grid")
    solver = request["solver"]
    fields(solver, ("backend", "seed", "samples", "max_depth", "interactions",
                    "synthetic_array", "loop_mode", "antenna"))
    if solver["backend"] != "llvm_ad_mono_polarized":
        raise ContractError("only explicit LLVM CPU backend supported")
    number(solver["seed"], 0, 2**32-1, True)
    number(solver["samples"], 1, 1000000, True)
    if type(solver["max_depth"]) is not int or solver["max_depth"] != 0:
        raise ContractError("Phase 0 empty-space proof supports depth zero")
    if (solver["interactions"] != INTERACTIONS or
            any(type(v) is not bool for v in solver["interactions"].values())):
        raise ContractError("only LOS is validated by this worker version")
    if (solver["synthetic_array"] is not True or solver["loop_mode"] != "evaluated"
            or solver["antenna"] != "isotropic-single-V"):
        raise ContractError("unsupported array, antenna or loop mode")
    fields(request["limits"], ("timeout_s", "cpu_s"))
    number(request["limits"]["timeout_s"], 0.1, 120)
    number(request["limits"]["cpu_s"], 1, 120, True)
    return request


def _array(value, shape, leaf):
    if not shape:
        if not leaf(value):
            raise ContractError("invalid result array value")
    elif type(value) is not list or len(value) != shape[0]:
        raise ContractError("result array shape mismatch")
    else:
        for child in value:
            _array(child, shape[1:], leaf)


def _mask_matches(values, masks):
    if type(values) is list:
        return all(_mask_matches(v, m) for v, m in zip(values, masks))
    return masks == (values == 0)


def validate_result(response, request):
    """Reject correlated/checksummed but semantically malformed engine results."""
    from . import AUDITED_REVISION, ENGINE_PINS, WORKER_VERSION
    if (type(response) is not dict or type(response.get("schema_version")) is not int
            or response["schema_version"] != 1 or response.get("request_id") != request["request_id"]
            or response.get("request_sha256") != digest(request)
            or response.get("status") not in ("completed", "failed")):
        raise ContractError("response identity or status mismatch")
    if response["status"] == "failed":
        if type(response.get("error")) is not str or "data" in response:
            raise ContractError("failure contains prediction or no error")
        return
    versions = response.get("versions", {})
    source_pin = json.loads(Path(__file__).with_name("source_pin.json").read_text())
    if (type(versions) is not dict or any(versions.get(k) != v for k, v in ENGINE_PINS.items())
            or versions.get("worker") != WORKER_VERSION
            or versions.get("backend") != "llvm_ad_mono_polarized"
            or versions.get("audited_source_revision") != AUDITED_REVISION
            or versions.get("source_pin_sha256") != digest(source_pin)
            or versions.get("audited_python_source_verified") is not True):
        raise ContractError("unverified engine versions")
    python_version = versions.get("python")
    if type(python_version) is not str or not re.fullmatch(r"3\.[0-9]{2,3}\.[0-9]{1,3}", python_version):
        raise ContractError("missing or malformed Python runtime version")
    if int(python_version.split(".")[1]) < 10:
        raise ContractError("unsupported Python runtime version")
    for field in ("os", "machine"):
        value = versions.get(field)
        if (type(value) is not str or not 1 <= len(value) <= 256 or not value.strip()
                or any(ord(c) < 32 or ord(c) == 127 for c in value)):
            raise ContractError("missing or malformed runtime provenance: " + field)
    native_hash = versions.get("llvm_library_sha256")
    if type(native_hash) is not str or not re.fullmatch(r"[0-9a-f]{64}", native_hash):
        raise ContractError("missing or malformed LLVM native-build hash")
    if request["operation"] == "capabilities":
        if type(response.get("capabilities")) is not dict or response["capabilities"].get("cpu_llvm") is not True:
            raise ContractError("CPU backend not available")
        return
    data = response.get("data")
    if (type(data) is not dict or response.get("data_sha256") != digest(data)
            or response.get("scene_sha256") != request["scene_sha256"]
            or response.get("solver") != request["solver"]
            or any(response.get(k) != request[k] for k in ("frequency_hz", "bandwidth_hz", "temperature_k", "profile_revision"))
            or response.get("noise_model") != "not_used_for_path_gain"):
        raise ContractError("result provenance/checksum mismatch")
    finite = lambda x: type(x) in (int, float) and math.isfinite(x)
    power = lambda x: finite(x) and x >= 0
    mask = lambda x: type(x) is bool
    ntx, nrx = len(request["transmitters"]), len(request["receivers"])
    if request["operation"] == "validate_scene":
        if data != {"kind": "scene_diagnostics", "valid": True,
                    "geometry": "empty_space", "geometry_object_count": 0}:
            raise ContractError("invalid scene diagnostics")
        return
    if data.get("transmitter_ids") != [x["id"] for x in request["transmitters"]]:
        raise ContractError("transmitter result identity mismatch")
    if request["operation"] == "radio_map":
        grid = request["grid"]
        nx, ny = [round(s/c) for s, c in zip(grid["size_m"], grid["cell_size_m"])]
        shape = [ntx, ny, nx]
        if (data.get("kind") != "planar_radio_map" or data.get("shape") != shape
                or data.get("units") != "linear_power_ratio" or data.get("grid") != grid
                or data.get("precision") != "float32"
                or data.get("combination") != "monte_carlo_cell_average_power"
                or data.get("orientation_rad") != [0, 0, 0]
                or data.get("axis_order") != ["transmitter", "y", "x"]):
            raise ContractError("radio map semantics mismatch")
        _array(data.get("cell_centers_m"), [ny, nx, 3], finite)
        for y, row in enumerate(data["cell_centers_m"]):
            for x, center in enumerate(row):
                expected = [grid["center_m"][0]-grid["size_m"][0]/2+(x+.5)*grid["cell_size_m"][0],
                            grid["center_m"][1]-grid["size_m"][1]/2+(y+.5)*grid["cell_size_m"][1],
                            grid["center_m"][2]]
                if any(abs(a-b) > 0.002 for a, b in zip(center, expected)):
                    raise ContractError("radio map cell coordinate mismatch")
    else:
        shape = [nrx, 1, ntx, 1]
        cshape = data.get("coefficient_shape")
        if (data.get("kind") != "point_paths" or data.get("path_gain_shape") != shape
                or data.get("units") != {"path_gain": "linear_power_ratio", "delay": "s", "coefficient": "complex_amplitude_ratio"}
                or data.get("combination") != "sum_squared_path_amplitudes"
                or data.get("precision") != "complex64"
                or data.get("receiver_ids") != [x["id"] for x in request["receivers"]]
                or type(cshape) is not list or len(cshape) != 6 or cshape[:4] != shape
                or type(cshape[4]) is not int or cshape[4] != 1 or cshape[5] != 1
                or data.get("delay_shape") != [nrx, ntx, cshape[4]]):
            raise ContractError("point path semantics mismatch")
        _array(data.get("coefficients_real"), cshape, finite)
        _array(data.get("coefficients_imag"), cshape, finite)
        _array(data.get("delays_s"), [nrx, ntx, cshape[4]], lambda x: finite(x) and x > 0)
    _array(data.get("path_gain"), shape, power)
    _array(data.get("no_data_mask"), shape, mask)
    if not _mask_matches(data["path_gain"], data["no_data_mask"]):
        raise ContractError("no-data mask disagrees with zero sampled path gain")
    if request["operation"] == "path_query":
        for rx in range(nrx):
            for tx in range(ntx):
                distance = math.dist(request["receivers"][rx]["position_m"],
                                     request["transmitters"][tx]["position_m"])
                delay = data["delays_s"][rx][tx][0]
                # Restricted empty-space LOS only: absolute delay is distance/c.
                # The 2 mm term bounds float32 coordinate rounding at +/-10 km.
                if not math.isclose(delay, distance/299792458, rel_tol=1e-5,
                                    abs_tol=0.002/299792458):
                    raise ContractError("LOS delay disagrees with transmitter/receiver distance")
                real = data["coefficients_real"][rx][0][tx][0][0][0]
                imag = data["coefficients_imag"][rx][0][tx][0][0][0]
                gain = data["path_gain"][rx][0][tx][0]
                coefficient_power = real*real + imag*imag
                if not math.isfinite(coefficient_power) or abs(gain-coefficient_power) > 1e-5*max(gain, coefficient_power):
                    raise ContractError("path gain disagrees with squared complex coefficient")
