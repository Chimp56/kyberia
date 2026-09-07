#!/usr/bin/env python3
"""Original research fixtures, never production RF calculations or measurements."""

import argparse
import hashlib
import json
import math
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
VERSION = "kyberia-synthetic/1"
SEED = 42


def canonical_bytes(value):
    return (json.dumps(value, indent=2, sort_keys=True, allow_nan=False) + "\n").encode()


def quantity(value, unit, tolerance=0.000001):
    return {"value": value, "unit": unit, "absolute_tolerance": tolerance}


def scene(identifier, inputs, expected, assumptions, references):
    return {
        "id": identifier,
        "evidence_class": "synthetic",
        "generator": VERSION,
        "source": "kyberia-original-synthetic-v1",
        "coordinate_frame": "building-local-right-handed-z-up-meters",
        "inputs": inputs,
        "expected": expected,
        "assumptions": assumptions,
        "requirements": references,
    }


def canonical_scenes():
    """Stored decimal baselines are independent of the production implementation."""
    scenes = []
    loss = {2400000000: 60.052008056116, 5000000000: 66.427183308604,
            6000000000: 68.010808229556}
    common = {
        "tx_position_m": [0, 0, 1.5], "rx_position_m": [10, 0, 1.5],
        "tx_conducted_dbm": 20, "tx_gain_dbi": 0, "rx_gain_dbi": 0,
        "cable_loss_db": 0, "path_loss_exponent": 2,
    }
    for frequency, base in loss.items():
        for name, penalties in [("open", []), ("one-wall", [3]), ("two-wall", [3, 7])]:
            inputs = dict(common, frequency_hz=frequency,
                          crossings=[{"x_m": 3 + 3 * i,
                                      "synthetic_insertion_loss_db": penalty}
                                     for i, penalty in enumerate(penalties)])
            scenes.append(scene(
                "%s-%s" % (name, frequency), inputs,
                {"path_loss": quantity(base + sum(penalties), "dB"),
                 "rss": quantity(20 - base - sum(penalties), "dBm")},
                ["Far-field Friis, isotropic antennas, speed of light 299792458 m/s.",
                 "Crossing penalties are artificial ideal attenuators, not material presets.",
                 "P0/P1 additive-loss fixture; no reflection or multipath claim."],
                ["7.10", "8.4", "16.5", "Appendix I/TST-003"]))
    scenes.append(scene(
        "material-frequency-comparison",
        {"distance_m": 10, "tx_dbm": 20,
         "frequency_hz": [2400000000, 5000000000, 6000000000],
         "materials": {"artificial-A": [3, 6, 9], "artificial-B": [7, 10, 13]}},
        {"A_rss_dbm": [-43.052008056116, -52.427183308604, -57.010808229556],
         "B_rss_dbm": [-47.052008056116, -56.427183308604, -61.010808229556],
         "B_minus_A": quantity(-4, "dB"), "absolute_tolerance_db": 0.000001},
        ["One ideal attenuator at equal distance; arbitrary frequency curves test mapping.",
         "These are not concrete, glass, or vendor material measurements."], ["8.3", "16.5"]))
    scenes.append(scene(
        "slab-versus-opening",
        {"tx_position_m": [0, 0, 1], "rx_position_m": [0, 0, 11],
         "floor_elevations_m": [0, 5, 10], "slab_z_m": 5,
         "opening_xy_m": [[-1, -1], [1, -1], [1, 1], [-1, 1]],
         "frequency_hz": 5000000000, "tx_dbm": 20, "slab_loss_db": 12},
        {"closed_slab_rss": quantity(-58.427183308604, "dBm"),
         "open_slab_rss": quantity(-46.427183308604, "dBm"),
         "opening_minus_closed": quantity(12, "dB")},
        ["One slab at z=5; the floor elevations are references, not extra obstructions.",
         "Opening variant removes the slab at the direct crossing; arbitrary 12 dB insertion."],
        ["8.2", "16.3", "16.5", "18.7/PREB-003"]))
    scenes.append(scene(
        "single-reflection-geometry",
        {"tx_position_m": [-2, 1, 1], "rx_position_m": [2, 1, 1],
         "reflector_plane": "y=0", "specular_point_m": [0, 0, 1]},
        {"reflected_path_length": quantity(4.472135955, "m"),
         "path_coefficient": None, "radio_map_rss": None},
        ["Image-source geometry only; interaction coefficients and radio map require the Sionna runtime gate.",
         "No invented electromagnetic reflection result or competing ray tracer."], ["16.5", "18.7/PREB-010"]))
    scenes.append(scene(
        "knife-edge-geometry",
        {"tx_position_m": [-5, 0, 1], "rx_position_m": [5, 0, 1],
         "screen_plane": "x=0", "screen_top_z_m": 1,
         "edge_direction": [0, 1, 0], "frequency_hz": 5000000000},
        {"grazing_direct_path_length": quantity(10, "m"),
         "path_coefficient": None, "radio_map_rss": None},
        ["A canonical grazing screen geometry for a pinned Sionna diffraction run.",
         "Field baseline deliberately unknown until independent analytic/upstream runtime validation."],
        ["16.5", "18.7/PREB-010"]))
    scenes.append(scene(
        "directional-antenna-rotation",
        {"world_receiver_direction": [1, 0, 0], "boresight_axis": "+x",
         "yaw_degrees_ccw_about_z": [0, 90, 180, 270, 360],
         "gain_model": "G_dBi(theta) = 3*cos(theta)", "isotropic_rss_dbm": -60,
         "off_axis_case": {"world_receiver_direction": [0.7071067811865476, 0.7071067811865476, 0],
                           "yaw_degrees_ccw_about_z": [45, -45]}},
        {"rss_dbm": [-57, -60, -63, -60, -57], "absolute_tolerance_db": 0.000001,
         "off_axis_rss_dbm": [-57, -60]},
        ["Original analytic directional test pattern; no vendor antenna or radiated-power normalization claim.",
         "Active rotations in right-handed z-up frame; gain uses local arrival/departure direction."],
        ["8.8", "16.3", "18.7/PREB-004"]))
    for label, coupling, interference in [("co-channel", 1, -60), ("adjacent-channel", 0.01, -80)]:
        scenes.append(scene(
            label + "-pair", {"desired_dbm": -50, "interferer_dbm": -60,
                                  "interferer_activity": 1, "coupling": coupling,
                                  "noise_dbm": None, "noise_reason": "not_observable"},
            {"effective_interference": quantity(interference, "dBm"),
             "sir": quantity(-50 - interference, "dB"), "sinr": None},
            ["Explicit synthetic coupling coefficient supplied to linear-power composition.",
             "Not a calibrated spectral mask, MAC contention, or universal adjacent-channel rule.",
             "Unknown noise forbids numerical measured SINR."], ["7.10", "7.11", "16.5"]))
    scenes.append(scene(
        "hidden-node-like-topology",
        {"rx_power_dbm": {"A_to_B": -95, "B_to_A": -95, "A_to_R": -55, "B_to_R": -55},
         "scenario_cca_threshold_dbm": -82, "simultaneous_transmission": True},
        {"A_senses_B": False, "B_senses_A": False,
         "receiver_equal_power_sir": quantity(0, "dB"), "collision_probability": None},
        ["Artificial asymmetric topology tests sensing versus receiver interference.",
         "Threshold is a scenario input, not a regulatory or chipset default.",
         "No MAC scheduler means collision probability is unknown."], ["Appendix I/PHY-007"]))
    for identifier, rssi, lan, wan, occupancy in [
            ("low-rssi-high-throughput", -77, 120, 80, 0.1),
            ("high-rssi-congested", -40, 8, 7, 0.95),
            ("healthy-lan-slow-wan", -50, 250, 2, 0.1)]:
        scenes.append(scene(
            identifier,
            {"synthetic_rssi_dbm": rssi, "active_window_seconds": [0, 5],
             "lan_endpoint": {"tier": "LAN_REFERENCE", "throughput_mbps": lan},
             "internet_endpoint": {"tier": "INTERNET_CONTROL", "throughput_mbps": wan},
             "scenario_occupancy_probability": occupancy},
            {"rssi_to_goodput_function": None, "preserve_lan_mbps": lan,
             "preserve_internet_mbps": wan, "may_assert_proven_root_cause": False},
            ["Fabricated independent semantic inputs, not measured captures or a PHY curve.",
             "A diagnostic hypothesis must preserve endpoint tier and contradictory evidence."],
            ["7.15", "7.17", "16.8", "6.9/REQ-004"]))
    scenes.append(scene(
        "uplink-limited-client", {"ap_tx_dbm": 25, "client_tx_dbm": 10,
                                   "path_loss_db": 85, "antenna_gains_db": 0,
                                   "scenario_required_bidirectional_dbm": -67},
        {"downlink": quantity(-60, "dBm"), "uplink": quantity(-75, "dBm"),
         "bidirectional_min": quantity(-75, "dBm"), "compliance": "FAIL"},
        ["Reciprocal path loss; unequal transmit powers; threshold is illustrative user policy."],
        ["7.16", "8.9", "18.7/PREB-005"]))
    scenes.append(scene(
        "measurement-gap", {"sample_points_m": [[0, 0], [4, 0], [0, 4]],
                            "sample_rssi_dbm": [-40, -60, -80],
                            "queries_m": [[1, 1], [4, 4]], "extrapolate": False},
        {"values_dbm": [-55, None], "classes": ["interpolated", "outside_evidence_support"],
         "absolute_tolerance_db": 0.000001, "outside_counts_as_pass": False},
        ["One nondegenerate triangle; no extrapolation outside convex hull."], ["7.24", "16.4"]))
    scenes.append(scene(
        "roaming-boundary", {"times_seconds": [0, 1, 2, 3], "path_x_m": [0, 1, 2, 3],
                             "ap_A_dbm": [-55, -65, -70, -74], "ap_B_dbm": [-75, -68, -66, -60],
                             "initial_association": "A", "hysteresis_db": 5},
        {"association": ["A", "A", "A", "B"], "transition_time_interval_seconds": [2, 3],
         "transition_position_interval_m": [2, 3], "authentication_duration_ms": None},
        ["Toy policy: roam when candidate minus current >=5 dB, no dwell condition.",
         "Sampling bounds a transition interval; it does not measure authentication timing."],
        ["7.19", "8.13", "16.8"]))
    scenes.append(scene(
        "multi-floor-candidate-optimum",
        {"demand_cells": [{"id": "L", "position_m": [0, 0, 1]},
                          {"id": "U", "position_m": [0, 0, 4]}],
         "candidates": [{"id": "A", "floor": 0, "covers": ["L"], "cost": 2},
                        {"id": "B", "floor": 1, "covers": ["U"], "cost": 2},
                        {"id": "C", "floor": 0, "covers": ["L", "U"], "cost": 5}],
         "objective_lexicographic": ["uncovered_cells", "total_cost", "ap_count", "sorted_candidate_ids"]},
        {"selected": ["A", "B"], "total_cost": 4, "ap_count": 2,
         "all_feasible_subsets": [["A", "B"], ["C"], ["A", "C"], ["B", "C"], ["A", "B", "C"]]},
        ["Exact finite set cover, synthetic coefficients; no full-model or regulatory feasibility claim.",
         "Enumerating all eight subsets proves the optimum for this stated objective."],
        ["9.15", "16.11", "18.8/OPTB-003"]))
    return {"schema_version": 1, "generator": VERSION, "seed": SEED,
            "evidence_class": "synthetic", "scenes": scenes,
            "tolerance_policy": "1e-6 dB bounds serialization/f64 arithmetic only; not physical accuracy."}


def triangle_value(vertices, values, point):
    """Clean-room determinant formula, only for a single triangle research oracle."""
    if len(vertices) != 3 or len(values) != 3 or len(point) != 2:
        raise ValueError("exactly three 2D vertices and scalar values required")
    if any(len(vertex) != 2 for vertex in vertices):
        raise ValueError("2D vertices required")
    if not all(math.isfinite(x) for row in vertices + [point, values] for x in row):
        raise ValueError("finite inputs required")
    (ax, ay), (bx, by), (cx, cy) = vertices
    px, py = point
    determinant = (by - cy) * (ax - cx) + (cx - bx) * (ay - cy)
    if abs(determinant) < 1e-12:
        raise ValueError("degenerate triangle")
    a = ((by - cy) * (px - cx) + (cx - bx) * (py - cy)) / determinant
    b = ((cy - ay) * (px - cx) + (ax - cx) * (py - cy)) / determinant
    c = 1 - a - b
    if min(a, b, c) < -1e-12:
        return None
    return a * values[0] + b * values[1] + c * values[2]


def tin_fixture():
    return {
        "schema_version": 1, "evidence_class": "synthetic", "generator": VERSION,
        "source": "kyberia-original-tin-v1", "reference": "plan.md Appendix I wifiheatmap oracle",
        "upstream_audited_revision": "4602ce27903f3231567575feff3a1bffd962777b",
        "upstream_execution": "NOT_RUN", "source_code_copied": False,
        "coordinate_unit": "m", "value_unit": "dBm", "absolute_tolerance": 0.000001,
        "vertices": [[0, 0], [4, 0], [0, 4]],
        "selected_bss_values": [[-40, -45], [-70, -60], [-80, -90]],
        "selected_bss_aggregation": "max_per_position",
        "aggregated_values": [-40, -60, -80],
        "triangles": [[0, 1, 2]],
        "queries": [{"xy": [0, 0], "value": -40, "class": "synthetic_sample"},
                    {"xy": [1, 1], "value": -55, "class": "interpolated"},
                    {"xy": [2, 2], "value": -70, "class": "interpolated"},
                    {"xy": [4, 4], "value": None, "class": "outside_evidence_support"}],
        "scope": "One triangle is uniquely Delaunay; no general triangulation implementation.",
        "workflow": ["import original map", "calibrate in meters", "select scan/connected/iperf mode",
                     "select position", "collect capture window", "associate result to selected position",
                     "repeat three noncollinear points", "select BSS values", "render inside hull",
                     "save and reopen preserving position/value/mode", "cancel leaves incomplete window"],
        "active_semantics": {"endpoint_tier_required": True, "failed_throughput_value": None,
                             "retransmit_unit": "count", "scan_is_not_iperf": True},
    }


def survey_fixture(seed=SEED):
    """Seeded integer LCG jitter; documented bitwise-stable across Python platforms."""
    if type(seed) is not int or not 0 <= seed < 2 ** 32:
        raise ValueError("seed must be an unsigned 32-bit integer")
    state = seed
    samples = []
    for index in range(25):
        x, y = index % 5, index // 5
        state = (1664525 * state + 1013904223) % (2 ** 32)
        jitter = (state % 7 - 3) / 2
        truth = -40 - 2 * x - 3 * y - (12 if x >= 3 else 0)
        samples.append({"id": "synthetic-%02d" % index,
                        "evidence_class": "synthetic", "position_m": [x, y, 1.5],
                        "frame": "floor-0", "truth_dbm": truth,
                        "synthetic_rssi_dbm": truth + jitter,
                        "synthetic_position_covariance_m2": [0.04, 0, 0, 0.04],
                        "monotonic_ns": index * 1000000000,
                        "clock": "synthetic-session", "frequency_hz": 5000000000,
                        "noise_dbm": None, "noise_reason": "not_observable",
                        "fold": "room-east" if x >= 3 else "room-west",
                        "sensor": "synthetic-radio", "adapter_version": VERSION,
                        "calibration": "not_applicable_synthetic", "source": "kyberia-original-synthetic-v1"})
    return {"schema_version": 1, "generator": VERSION, "seed": seed,
            "evidence_class": "synthetic", "samples": samples,
            "position_covariance_layout": {"field": "synthetic_position_covariance_m2",
                                           "shape": [2, 2], "axes": ["x", "y"], "order": "row-major",
                                           "vertical_assumption": "z is fixed at 1.5 m by synthetic construction; no measured vertical precision claim"},
            "truth_model": "-40 - 2*x - 3*y - (12 if x>=3 else 0) dBm",
            "jitter_model": "LCG32(1664525,1013904223); (state%7-3)/2 dB; not Gaussian",
            "validation": "Hold out entire east/west rooms; no random nearby train/test split.",
            "warning": "Test-only original artificial field; not RF measurement or calibrated material data."}


def assets():
    return {"fixtures/synthetic-scenes/canonical-v1.json": canonical_scenes(),
            "fixtures/synthetic-scenes/survey-v1.json": survey_fixture(),
            "fixtures/wifiheatmap-oracle/tin-v1.json": tin_fixture()}


def check(root=ROOT):
    errors = []
    for path, document in assets().items():
        if not (root / path).is_file() or (root / path).read_bytes() != canonical_bytes(document):
            errors.append("fixture drift: " + path)
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["generate", "check", "hashes"])
    args = parser.parse_args()
    if args.action == "generate":
        for path, document in assets().items():
            target = ROOT / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(canonical_bytes(document))
    elif args.action == "check":
        errors = check()
        print("\n".join(errors) if errors else "PASS: original synthetic fixtures match generator")
        return 1 if errors else 0
    else:
        print(json.dumps({p: hashlib.sha256(canonical_bytes(d)).hexdigest()
                          for p, d in assets().items()}, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
