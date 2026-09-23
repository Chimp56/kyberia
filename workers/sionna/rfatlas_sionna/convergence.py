"""Bounded, support-only Monte Carlo diagnostics for ordinary radio-map jobs.

This module imports the worker client and contract, but never imports Sionna.
It runs repeated ordinary ``radio_map`` requests when explicitly invoked with a
Python executable.  Its summaries characterize repeat variability for one
reported runtime; they are not calibrated uncertainty or an acceptance gate.
"""

from copy import deepcopy
from datetime import datetime
import math
import re
import statistics

from .client import run as run_worker
from .contract import (ContractError, MAX_LOG_BYTES, MAX_REQUEST_BYTES,
                       MAX_RESULT_BYTES, canonical_bytes, digest, validate,
                       validate_result)


MAX_BUDGETS = 8
MAX_SEEDS = 8
MAX_RUNS = 32
MAX_CELLS = 4096
MAX_SUMMARY_VALUES = 100_000
MAX_OUTPUT_BYTES = 8 * 1024 * 1024
MAX_AGGREGATE_TRANSMITTER_SAMPLES = 32_000_000
_HEX_256 = re.compile(r"[0-9a-f]{64}\Z")


class ConvergenceError(ValueError):
    """A request, worker result, or bound is invalid for this diagnostic."""


class ConvergenceCancelled(ConvergenceError):
    """The caller cancelled the sweep; no subsequent worker job is started."""


def _finite_nonnegative(value):
    return type(value) in (int, float) and math.isfinite(value) and value >= 0


def _check_cancel(cancel, stage):
    if cancel is None:
        return
    try:
        cancelled = cancel.is_set()
    except Exception as exc:
        raise ConvergenceError("cancel token could not be checked") from exc
    if type(cancelled) is not bool:
        raise ConvergenceError("cancel token is_set() must return bool")
    if cancelled:
        raise ConvergenceCancelled("sweep cancelled " + stage)


def _check_options(request, sample_budgets, seeds):
    if type(request) is not dict or request.get("operation") != "radio_map":
        raise ConvergenceError("only ordinary radio_map requests are supported")
    try:
        validate(request)
        if len(canonical_bytes(request)) > MAX_REQUEST_BYTES:
            raise ConvergenceError("base request exceeds the worker request bound")
    except (ContractError, TypeError, ValueError, RecursionError) as exc:
        if isinstance(exc, ConvergenceError):
            raise
        raise ConvergenceError("invalid base request: " + str(exc)) from exc

    if type(sample_budgets) not in (list, tuple) or not 2 <= len(sample_budgets) <= MAX_BUDGETS:
        raise ConvergenceError("sample budgets must contain 2 to 8 entries")
    if any(type(n) is not int or not 1 <= n <= 1_000_000 for n in sample_budgets):
        raise ConvergenceError("sample budgets must be integers in the worker range")
    if any(a >= b for a, b in zip(sample_budgets, sample_budgets[1:])):
        raise ConvergenceError("sample budgets must be strictly increasing")

    if type(seeds) not in (list, tuple) or not 2 <= len(seeds) <= MAX_SEEDS:
        raise ConvergenceError("seeds must contain 2 to 8 entries")
    if any(type(seed) is not int or not 0 <= seed <= 2**32 - 1 for seed in seeds):
        raise ConvergenceError("seeds must be integers in the worker range")
    if len(set(seeds)) != len(seeds):
        raise ConvergenceError("seeds must be unique")
    if len(sample_budgets) * len(seeds) > MAX_RUNS:
        raise ConvergenceError("sweep exceeds the run bound")

    grid = request["grid"]
    nx, ny = [round(size / cell) for size, cell in
              zip(grid["size_m"], grid["cell_size_m"])]
    cells = nx * ny
    if not 1 <= cells <= MAX_CELLS:
        raise ConvergenceError("sweep exceeds the per-map cell bound")
    ntx = len(request["transmitters"])
    aggregate_samples = sum(sample_budgets) * len(seeds) * ntx
    if aggregate_samples > MAX_AGGREGATE_TRANSMITTER_SAMPLES:
        raise ConvergenceError("sweep exceeds the aggregate transmitter-sample bound")
    summary_values = (4 * len(sample_budgets) - 2) * ntx * cells
    if summary_values > MAX_SUMMARY_VALUES:
        raise ConvergenceError("sweep exceeds the summary-value bound")
    return nx, ny, cells, ntx


def _request_for_run(base_request, samples, seed):
    request = deepcopy(base_request)
    identity = digest({"base_request_sha256": digest(base_request),
                       "samples": samples, "seed": seed})
    request["request_id"] = "cv-" + identity[:24]
    request["solver"]["samples"] = samples
    request["solver"]["seed"] = seed
    return request


def _validate_envelope(envelope, request):
    if type(envelope) is not dict:
        raise ConvergenceError("worker client returned a malformed envelope")
    if (envelope.get("schema_version") != 1
            or type(envelope.get("schema_version")) is not int
            or envelope.get("status") != "completed"
            or envelope.get("request_id") != request["request_id"]
            or envelope.get("request_sha256") != digest(request)
            or envelope.get("returncode") != 0
            or type(envelope.get("returncode")) is not int
            or not _HEX_256.fullmatch(envelope.get("log_sha256", ""))
            or envelope.get("cleanup_error") is not None
            or envelope.get("cancel_error") is not None
            or not _finite_nonnegative(envelope.get("elapsed_s"))
            or type(envelope.get("resource_limits")) is not dict):
        raise ConvergenceError("worker envelope identity, status, or provenance is invalid")
    timestamps = []
    for field in ("started_utc", "ended_utc"):
        value = envelope.get(field)
        if type(value) is not str or len(value) > 64:
            raise ConvergenceError("worker envelope is missing " + field)
        try:
            parsed = datetime.fromisoformat(value)
            if parsed.tzinfo is None:
                raise ValueError("timezone is required")
            timestamps.append(parsed)
        except ValueError as exc:
            raise ConvergenceError("worker envelope has malformed " + field) from exc
    if timestamps[1] < timestamps[0]:
        raise ConvergenceError("worker envelope timestamps are reversed")
    log = envelope.get("log")
    if type(log) is not str:
        raise ConvergenceError("worker envelope log is malformed or over budget")
    try:
        if len(log.encode("utf-8")) > MAX_LOG_BYTES:
            raise ConvergenceError("worker envelope log is malformed or over budget")
    except UnicodeError as exc:
        raise ConvergenceError("worker envelope log is malformed or over budget") from exc
    result = envelope.get("result")
    if type(result) is not dict:
        raise ConvergenceError("worker envelope does not contain a result object")
    try:
        result_bytes = canonical_bytes(result)
    except (ContractError, TypeError, ValueError, OverflowError, RecursionError) as exc:
        raise ConvergenceError("worker result is not canonical JSON") from exc
    if len(result_bytes) > MAX_RESULT_BYTES:
        raise ConvergenceError("worker result exceeds the result byte bound")
    try:
        validate_result(result, request)
    except (ContractError, TypeError, ValueError, RecursionError) as exc:
        raise ConvergenceError("worker result failed contract validation: " + str(exc)) from exc
    if type(result) is not dict or result.get("status") != "completed":
        raise ConvergenceError("worker result is not completed")
    if not _finite_nonnegative(result.get("elapsed_engine_s")):
        raise ConvergenceError("worker result has invalid engine runtime")
    warnings = result.get("warnings")
    if type(warnings) is not list or any(type(item) is not str for item in warnings):
        raise ConvergenceError("worker result has malformed warnings")
    return result


def _flatten_map(values, shape):
    """Return transmitter/y/x values in canonical row-major order."""
    ntx, ny, nx = shape
    if type(values) is not list or len(values) != ntx:
        raise ConvergenceError("path-gain transmitter dimension changed")
    flat = []
    for tx in values:
        if type(tx) is not list or len(tx) != ny:
            raise ConvergenceError("path-gain y dimension changed")
        for row in tx:
            if type(row) is not list or len(row) != nx:
                raise ConvergenceError("path-gain x dimension changed")
            for value in row:
                if type(value) not in (int, float) or not math.isfinite(value) or value < 0:
                    raise ConvergenceError("path gain must be finite and nonnegative")
                flat.append(float(value))
    return flat


def _map_matrix(flat, ntx, ny, nx):
    result = []
    offset = 0
    for _ in range(ntx):
        transmitter = []
        for _ in range(ny):
            transmitter.append(flat[offset:offset + nx])
            offset += nx
        result.append(transmitter)
    return result


def _mean_and_standard_error(samples):
    try:
        mean = math.fsum(samples) / len(samples)
        standard_error = statistics.stdev(samples) / math.sqrt(len(samples))
    except (OverflowError, ValueError) as exc:
        raise ConvergenceError("non-finite convergence statistic") from exc
    if not math.isfinite(mean) or not math.isfinite(standard_error):
        raise ConvergenceError("non-finite convergence statistic")
    return mean, standard_error


def _runtime_summary(values):
    if len(values) < 2 or any(not _finite_nonnegative(value) for value in values):
        raise ConvergenceError("runtime observations are invalid")
    try:
        spread = statistics.stdev(values)
    except (OverflowError, ValueError) as exc:
        raise ConvergenceError("runtime spread is non-finite") from exc
    if not math.isfinite(spread):
        raise ConvergenceError("runtime spread is non-finite")
    return {"unit": "s", "min": min(values), "median": statistics.median(values),
            "max": max(values), "sample_standard_deviation": spread}


def run_sweep(request, sample_budgets, seeds, python_executable, *, cancel=None,
              runner=None):
    """Run a bounded repeated-seed sweep through the normal worker client.

    ``cancel`` is an event-like token with ``is_set()``; it is propagated into
    every client call and checked between calls. ``runner`` is injectable for
    algorithm tests; production use should omit it to invoke
    :func:`rfatlas_sionna.client.run`. Each emitted run request differs from
    ``request`` only in request ID, seed, and samples.
    """
    if cancel is not None and not callable(getattr(cancel, "is_set", None)):
        raise ConvergenceError("cancel token must provide is_set()")
    nx, ny, cells, ntx = _check_options(request, sample_budgets, seeds)
    _check_cancel(cancel, "before the first worker call")
    execute = run_worker if runner is None else runner
    if not callable(execute):
        raise ConvergenceError("runner must be callable")

    base_semantics = deepcopy(request)
    base_semantics.pop("request_id")
    base_semantics["solver"].pop("samples")
    base_semantics["solver"].pop("seed")
    shape = (ntx, ny, nx)
    budget_runs = {budget: [] for budget in sample_budgets}
    runtimes = {budget: [] for budget in sample_budgets}
    provenance_signature = None
    run_records = []

    for budget in sample_budgets:
        for seed in seeds:
            _check_cancel(cancel, "before a worker call")
            run_request = _request_for_run(request, budget, seed)
            check_semantics = deepcopy(run_request)
            check_semantics.pop("request_id")
            check_semantics["solver"].pop("samples")
            check_semantics["solver"].pop("seed")
            if check_semantics != base_semantics:
                raise ConvergenceError("sweep modified an unrelated request field")
            try:
                envelope = execute(deepcopy(run_request), python_executable, cancel)
            except Exception as exc:
                try:
                    _check_cancel(cancel, "during a worker call")
                except ConvergenceCancelled as cancelled:
                    raise cancelled from exc
                raise ConvergenceError("radio_map worker call raised an exception") from exc
            _check_cancel(cancel, "during a worker call")
            result = _validate_envelope(envelope, run_request)
            data = result["data"]
            gains = _flatten_map(data.get("path_gain"), shape)
            if any(value == 0 for value in gains) or any(
                    any(mask for mask in row) for tx_map in data["no_data_mask"]
                    for row in tx_map):
                raise ConvergenceError("no-data cells cannot be included in convergence summaries")
            signature = (result["scene_sha256"], result["profile_revision"],
                         result["frequency_hz"], result["bandwidth_hz"],
                         result["temperature_k"], result["versions"],
                         result["capability_schema_version"], result["capabilities"])
            if provenance_signature is None:
                provenance_signature = deepcopy(signature)
            elif signature != provenance_signature:
                raise ConvergenceError("runtime or scene provenance changed within the sweep")
            elapsed = envelope["elapsed_s"]
            if type(elapsed) not in (int, float):
                raise ConvergenceError("worker runtime must be numeric")
            budget_runs[budget].append(gains)
            runtimes[budget].append(float(elapsed))
            run_records.append({
                "samples": budget,
                "seed": seed,
                "request": run_request,
                "request_sha256": envelope["request_sha256"],
                "started_utc": envelope["started_utc"],
                "ended_utc": envelope["ended_utc"],
                "worker_status": envelope["status"],
                "returncode": envelope["returncode"],
                "worker_elapsed_s": float(elapsed),
                "resource_limits": deepcopy(envelope["resource_limits"]),
                "log_sha256": envelope["log_sha256"],
                "cleanup_error": envelope["cleanup_error"],
                "cancel_error": envelope["cancel_error"],
                "result_provenance": {
                    "status": result["status"],
                    "request_id": result["request_id"],
                    "request_sha256": result["request_sha256"],
                    "data_sha256": result["data_sha256"],
                    "scene_sha256": result["scene_sha256"],
                    "elapsed_engine_s": result["elapsed_engine_s"],
                    "solver": result["solver"],
                    "versions": deepcopy(result["versions"]),
                    "capability_schema_version": result["capability_schema_version"],
                    "capabilities": deepcopy(result["capabilities"]),
                    "warnings": deepcopy(result["warnings"]),
                },
            })

    summaries = []
    flattened_means = []
    for budget in sample_budgets:
        maps = budget_runs[budget]
        means, errors = [], []
        for index in range(cells * ntx):
            mean, standard_error = _mean_and_standard_error([item[index] for item in maps])
            means.append(mean)
            errors.append(standard_error)
        flattened_means.append(means)
        summaries.append({
            "samples_per_run": budget,
            "replicate_count": len(seeds),
            "per_transmitter": [
                {"transmitter_id": transmitter["id"],
                 "mean_path_gain": _map_matrix(means[i*cells:(i+1)*cells], 1, ny, nx)[0],
                 "standard_error_path_gain": _map_matrix(errors[i*cells:(i+1)*cells], 1, ny, nx)[0]}
                for i, transmitter in enumerate(request["transmitters"])
            ],
            "runtime": _runtime_summary(runtimes[budget]),
        })

    adjacent = []
    for index, (lower, upper) in enumerate(zip(sample_budgets, sample_budgets[1:])):
        before, after = flattened_means[index], flattened_means[index + 1]
        delta = [new - old for old, new in zip(before, after)]
        relative = [abs(change) / max(abs(old), abs(new)) if max(abs(old), abs(new)) else 0.0
                    for old, new, change in zip(before, after, delta)]
        if not all(math.isfinite(value) for value in delta + relative):
            raise ConvergenceError("non-finite adjacent-budget change")
        adjacent.append({
            "from_samples_per_run": lower,
            "to_samples_per_run": upper,
            "unit": "linear_power_ratio",
            "mean_delta_path_gain": _map_matrix(delta, ntx, ny, nx),
            "symmetric_relative_change_fraction": _map_matrix(relative, ntx, ny, nx),
        })

    report = {
        "schema_version": 1,
        "kind": "sionna_cpu_convergence_diagnostic",
        "claim_scope": "support diagnostic only; not product acceptance",
        "base_request": deepcopy(request),
        "base_request_sha256": digest(request),
        "sample_budgets": list(sample_budgets),
        "seeds": list(seeds),
        "grid": deepcopy(request["grid"]),
        "shape": list(shape),
        "axis_order": ["transmitter", "y", "x"],
        "path_gain_unit": "linear_power_ratio",
        "standard_error": "sample standard deviation across distinct seeds divided by sqrt(replicate_count)",
        "relative_change": "abs(new_mean - old_mean) / max(abs(old_mean), abs(new_mean)); zero if both are zero",
        "runtime_scope": "within-sweep worker wall runtime on each recorded runtime; no cross-host parity claim",
        "raw_results_retained": False,
        "raw_result_retention_note": "per-run data_sha256 and worker provenance are retained; full maps are summarized and not copied into this bounded report",
        "summary_by_budget": summaries,
        "adjacent_budget_changes": adjacent,
        "runs": run_records,
    }
    try:
        encoded = canonical_bytes(report)
    except (ContractError, TypeError, ValueError, OverflowError) as exc:
        raise ConvergenceError("diagnostic report is not canonical JSON") from exc
    if len(encoded) > MAX_OUTPUT_BYTES:
        raise ConvergenceError("diagnostic report exceeds the output byte bound")
    return report
