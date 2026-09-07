#!/usr/bin/env python3
"""Original synthetic protocol fixtures; never captured wireless measurements."""
import copy
import json
from pathlib import Path

PROTOCOL = "kyberia.macos.collector/1"
SESSION = "00000000-0000-4000-8000-000000000001"
SCAN = "00000000-0000-4000-8000-000000000002"


def unknown(reason="source_did_not_provide"):
    return {"state": "unknown", "reason": reason}


def known(value):
    return {"state": "known", "value": value}


def source():
    return {"source_id": SESSION + ":en0", "interface_name": "en0",
            "identity_scope": "collector_process_and_interface", "physical_radio_id": unknown(),
            "driver_version": unknown(), "firmware_version": unknown(),
            "collector": "kyberia-macos-corewlan", "collector_version": "0.1.0",
            "collector_build": "sha256:" + "1" * 64, "source_kind": "native_api",
            "source_api": "CoreWLAN.CWInterface.scanForNetworks", "source_schema": PROTOCOL,
            "framework_version": "synthetic-framework", "os_version": "synthetic-macos-fixture"}


def fixture(status, authorization="authorized", sources=True, observation=False,
            redacted=True, command="scan", scan_started=True):
    hello = {key: value for key, value in source().items()
             if key in {"collector", "collector_version", "collector_build", "os_version"}}
    hello.update(kind="hello", command=command, evidence_origin="synthetic_fixture",
                 identifier_policy="redacted" if redacted else "explicit_unredacted",
                 max_observations=1, max_record_bytes=16384, timeout_seconds=20, clock_epoch=SESSION)
    descriptor = dict(source(), power_on=True, supported_channel_count=known(32), reported_band_enums=[1, 2, 3])
    capabilities = {
        "kind": "capabilities", "location_services_enabled": True, "location_authorization": authorization,
        "sources": [descriptor] if sources else [],
        "nearby_scan": {"state": "available" if sources and authorization == "authorized" else "unavailable",
                        "condition": "synthetic authorization/interface test"},
        "noise_dbm": {"state": "conditional", "condition": "source supplies usable reading"},
        "channel_width": {"state": "conditional", "condition": "source supplies recognized enum"},
        "monitor_frames": unknown("not_supported_by_collector"), "channel_dwell": unknown(),
        "channel_hopping_control": unknown("not_supported_by_collector"), "per_chain_signal": unknown(),
        "raw_payload_policy": "discard", "phy_metadata": unknown("not_implemented_by_collector"),
        "capture_timestamp": unknown(), "position": unknown("not_collected")}
    events = [hello, capabilities]
    if scan_started:
        events.append({"kind": "scan_started", "scan_id": SCAN, "source_id": SESSION + ":en0",
                       "api_started_monotonic_ns": "2900", "include_hidden": False})
    if observation:
        events.append({"kind": "scan_observation", "observation_id": "00000000-0000-4000-8000-000000000003",
                       "scan_id": SCAN, "source": source(), "evidence_class": "observed_api_result",
                       "api_window": {"start_monotonic_ns": "2900", "end_monotonic_ns": "3500"},
                       "bssid": unknown("redacted") if redacted else known("02:00:00:00:00:01"),
                       "ssid_octets_base64": unknown("redacted") if redacted else known("dGVzdC1vbmx5"),
                       "channel": known({"reported_channel_number": known(37), "band": known("6_ghz"),
                                         "width_mhz": unknown("unsupported_source_enum"), "raw_band_enum": 3,
                                         "raw_width_enum": 99, "frequency_hz": unknown(),
                                         "center_frequency_hz": unknown(), "puncturing": unknown()}),
                       "rssi_dbm": known(-61), "noise_dbm": unknown("invalid_or_unavailable_source_value"),
                       "measurement_method": "CoreWLAN.CWNetwork.rssiValue/noiseMeasurement", "calibration": "uncalibrated",
                       "result_age_seconds": unknown(), "dwell_seconds": unknown(),
                       "phy": unknown("not_implemented_by_collector"), "position": unknown("not_collected"),
                       "information_elements": unknown("not_retained_by_collector"),
                       "quality": ["capture_time_unknown", "scan_cache_age_unknown", "uncalibrated", "dwell_unknown"]})
    count = int(observation)
    events.append({"kind": "complete", "status": status, "reason": "synthetic_" + status,
                   "observation_count": count, "partial": status != "ok" and count > 0})
    for index, event in enumerate(events):
        event.update(protocol=PROTOCOL, session_id=SESSION, sequence=index,
                     time={"receipt_utc": "2026-09-07T00:00:00.000Z", "receipt_monotonic_ns": str((index + 1) * 1000),
                           "capture_time": unknown(), "clock_uncertainty_seconds": unknown("not_calibrated")})
    return events


def assets():
    return {"valid.ndjson": fixture("ok", observation=True, redacted=False),
            "partial.ndjson": fixture("partial", observation=True),
            "empty.ndjson": fixture("ok"),
            "error.ndjson": fixture("error"),
            "unsupported.ndjson": fixture("unsupported", sources=False, scan_started=False),
            "denied.ndjson": fixture("permission_required", authorization="denied", scan_started=False),
            "probe.ndjson": fixture("ok", authorization="not_determined", command="probe", scan_started=False)}


def encode(events):
    return b"".join((json.dumps(event, sort_keys=True, separators=(",", ":"), allow_nan=False) + "\n").encode()
                    for event in events)


if __name__ == "__main__":
    for name, events in assets().items():
        Path(__file__).with_name(name).write_bytes(encode(copy.deepcopy(events)))
