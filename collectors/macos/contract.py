"""Defensive decoder for the native macOS adapter's versioned NDJSON boundary.

This is adapter support code, not the canonical Kyberia observation model.
No CoreWLAN, OS object, or inferred RF quantity crosses this decoder.
"""
import base64
from datetime import datetime
import json
import math
import re

PROTOCOL = "kyberia.macos.collector/1"
MAX_RECORD_BYTES = 16384
MAX_RECORDS = 4164
MAX_STREAM_BYTES = MAX_RECORD_BYTES * MAX_RECORDS
UUID = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\Z")
MAC = re.compile(r"(?:[0-9A-Fa-f]{2}:){5}[0-9A-Fa-f]{2}\Z")
INTERFACE = re.compile(r"[A-Za-z0-9_]{1,64}\Z")
STATUSES = {"ok", "partial", "permission_required", "unsupported", "unavailable", "error", "timeout", "cancelled"}
UNKNOWN_REASONS = {"source_did_not_provide", "not_calibrated", "unsupported_source_enum",
                   "invalid_or_unavailable_source_value", "not_supported_by_collector",
                   "not_implemented_by_collector", "not_collected", "not_retained_by_collector", "redacted"}


class ContractError(ValueError):
    """Malformed, inconsistent, unsupported, or unbounded native stream."""


def require(condition, message):
    if not condition:
        raise ContractError(message)


def text(value, maximum=256):
    return isinstance(value, str) and 0 < len(value.encode("utf-8")) <= maximum


def integer(value, minimum, maximum):
    return type(value) is int and minimum <= value <= maximum


def bssid_value(value):
    if not isinstance(value, str) or MAC.fullmatch(value) is None:
        return False
    octets = bytes.fromhex(value.replace(":", ""))
    return octets[0] & 1 == 0 and any(octets)


def uint64(value):
    require(isinstance(value, str) and re.fullmatch(r"0|[1-9][0-9]{0,19}", value) is not None,
            "monotonic nanoseconds must be an unsigned decimal string")
    number = int(value)
    require(number <= 2 ** 64 - 1, "monotonic nanoseconds overflow")
    return number


def evidence(value, validator=None):
    require(isinstance(value, dict), "evidence must be an object")
    if value.get("state") == "unknown":
        require(set(value) == {"state", "reason"} and isinstance(value["reason"], str)
                and value["reason"] in UNKNOWN_REASONS, "invalid unknown reason")
        return None
    require(value.get("state") == "known" and set(value) == {"state", "value"}, "invalid known evidence")
    require(validator is not None and validator(value["value"]), "invalid or invented known value")
    return value["value"]


def timestamp(value):
    require(isinstance(value, dict), "timestamp must be an object")
    require(set(value) == {"receipt_utc", "receipt_monotonic_ns", "capture_time", "clock_uncertainty_seconds"},
            "timestamp schema mismatch")
    utc = value["receipt_utc"]
    require(isinstance(utc, str) and "T" in utc and utc.endswith("Z"), "receipt UTC timestamp required")
    try:
        date = datetime.fromisoformat(utc.replace("Z", "+00:00"))
        require(date.tzinfo is not None and date.utcoffset().total_seconds() == 0, "UTC required")
    except ValueError as error:
        raise ContractError("invalid UTC timestamp") from error
    evidence(value["capture_time"])
    evidence(value["clock_uncertainty_seconds"])
    return uint64(value["receipt_monotonic_ns"])


def source(value, session, hello):
    require(isinstance(value, dict), "source provenance required")
    for key in ["collector", "collector_version", "collector_build", "os_version"]:
        require(value.get(key) == hello[key], "source/hello provenance mismatch")
    require(value.get("source_kind") == "native_api" and value.get("source_schema") == PROTOCOL,
            "invalid source kind/version")
    require(value.get("source_api") == "CoreWLAN.CWInterface.scanForNetworks", "source API required")
    require(value.get("identity_scope") == "collector_process_and_interface", "radio identity scope required")
    interface = value.get("interface_name")
    require(isinstance(interface, str) and INTERFACE.fullmatch(interface) is not None, "invalid interface name")
    require(value.get("source_id") == session + ":" + interface, "source identity mismatch")
    require(text(value.get("framework_version")), "framework version required")
    for key in ["physical_radio_id", "driver_version", "firmware_version"]:
        evidence(value.get(key))
    return value["source_id"]


def channel(value):
    data = evidence(value, lambda item: isinstance(item, dict))
    if data is None:
        return
    evidence(data.get("reported_channel_number"), lambda n: integer(n, 1, 65535))
    band = evidence(data.get("band"), lambda b: b in {"2.4_ghz", "5_ghz", "6_ghz"})
    width = evidence(data.get("width_mhz"), lambda n: type(n) is int and n in {20, 40, 80, 160})
    for key in ["frequency_hz", "center_frequency_hz", "puncturing"]:
        evidence(data.get(key))
    require(integer(data.get("raw_band_enum"), 0, 65535) and integer(data.get("raw_width_enum"), 0, 65535),
            "raw channel enums required")
    require(band == {1: "2.4_ghz", 2: "5_ghz", 3: "6_ghz"}.get(data["raw_band_enum"]), "band enum mismatch")
    require(width == {1: 20, 2: 40, 3: 80, 4: 160}.get(data["raw_width_enum"]), "width enum mismatch")


def _json(line):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "duplicate JSON key")
            result[key] = value
        return result

    def finite(value):
        number = float(value)
        require(math.isfinite(number), "nonfinite JSON")
        return number

    def constant(_):
        raise ContractError("nonfinite JSON")

    try:
        return json.loads(line, object_pairs_hook=unique, parse_float=finite, parse_constant=constant)
    except (ValueError, UnicodeError, RecursionError) as error:
        raise ContractError("invalid JSON record") from error


def _decode_stream(data):
    """Return validated events, preserving unknowns and partial results unchanged.

    Consumers must preserve hello.evidence_origin when replaying synthetic
    fixtures, and inspect the final status before calling a survey complete.
    """
    require(isinstance(data, bytes) and 0 < len(data) <= MAX_STREAM_BYTES, "invalid stream size/type")
    require(data.endswith(b"\n"), "truncated final record")
    lines = data.splitlines(keepends=True)
    require(2 <= len(lines) <= MAX_RECORDS, "invalid record count")
    events = []
    hello = None
    session = None
    previous_time = 0
    capabilities = None
    sources = {}
    scans = {}
    observations = set()
    complete = None
    authorization_event = None
    for index, line in enumerate(lines):
        require(len(line) <= MAX_RECORD_BYTES and line.endswith(b"\n"), "oversized/truncated record")
        event = _json(line)
        require(isinstance(event, dict), "event must be an object")
        require(event.get("protocol") == PROTOCOL, "unsupported protocol version")
        require(type(event.get("sequence")) is int and event["sequence"] == index, "sequence mismatch")
        event_session = event.get("session_id")
        require(isinstance(event_session, str) and UUID.fullmatch(event_session) is not None, "session UUID required")
        if session is None:
            session = event_session
        require(session == event_session, "mixed process sessions")
        now = timestamp(event.get("time"))
        require(now >= previous_time, "reversed receipt monotonic time")
        previous_time = now
        kind = event.get("kind")
        require(complete is None, "event after completion")
        if kind == "hello":
            require(index == 0 and hello is None, "hello must be first and unique")
            require(event.get("collector") == "kyberia-macos-corewlan", "invalid collector")
            for key in ["collector_version", "os_version"]:
                require(text(event.get(key)), "missing version provenance")
            require(isinstance(event.get("collector_build"), str)
                    and re.fullmatch(r"sha256:[0-9a-f]{64}", event["collector_build"]) is not None,
                    "source build hash required")
            require(event.get("command") in {"probe", "scan", "authorize"}, "unsupported command")
            require(event.get("evidence_origin") in {"native_runtime", "synthetic_fixture"}, "origin required")
            require(event.get("identifier_policy") in {"redacted", "explicit_unredacted"}, "privacy policy required")
            require(integer(event.get("max_observations"), 1, 4096), "observation limit required")
            require(event.get("max_record_bytes") == MAX_RECORD_BYTES, "record limit mismatch")
            require(integer(event.get("timeout_seconds"), 1, 60), "deadline required")
            require(event.get("clock_epoch") == session, "clock epoch mismatch")
            hello = event
        else:
            require(hello is not None, "missing hello")
            if kind == "capabilities":
                require(capabilities is None and hello["command"] != "authorize", "unexpected capabilities")
                require(type(event.get("location_services_enabled")) is bool, "location availability required")
                require(event.get("location_authorization") in {"authorized", "denied", "restricted", "not_determined", "unknown"},
                        "invalid authorization state")
                entries = event.get("sources")
                require(isinstance(entries, list) and len(entries) <= 32, "invalid sources")
                for entry in entries:
                    identifier = source(entry, session, hello)
                    require(identifier not in sources and type(entry.get("power_on")) is bool, "duplicate/invalid source")
                    evidence(entry.get("supported_channel_count"), lambda n: integer(n, 0, 65535))
                    bands = entry.get("reported_band_enums")
                    require(isinstance(bands, list) and len(bands) <= 256
                            and all(integer(b, 0, 65535) for b in bands), "invalid reported bands")
                    sources[identifier] = entry
                require(event.get("raw_payload_policy") == "discard", "payload retention forbidden")
                capability = event.get("nearby_scan")
                require(isinstance(capability, dict) and capability.get("state") in {"available", "unavailable"}
                        and text(capability.get("condition")), "nearby scan capability missing")
                allowed = event["location_services_enabled"] and event["location_authorization"] == "authorized"
                expected = "available" if allowed and any(s["power_on"] for s in sources.values()) else "unavailable"
                require(capability["state"] == expected, "false scan capability")
                for key in ["noise_dbm", "channel_width"]:
                    require(isinstance(event.get(key), dict) and event[key].get("state") == "conditional"
                            and text(event[key].get("condition")), "optional capability missing")
                for key in ["monitor_frames", "channel_dwell", "channel_hopping_control", "per_chain_signal", "phy_metadata", "capture_timestamp", "position"]:
                    evidence(event.get(key))
                capabilities = event
            elif kind == "authorization":
                require(hello["command"] == "authorize" and index == 1, "unexpected authorization event")
                require(event.get("state") in {"authorized", "denied", "restricted", "not_determined", "unknown"}
                        and event.get("requested_by_operator") is True, "invalid authorization event")
                require(type(event.get("location_services_enabled")) is bool, "location availability required")
                require(event.get("prompt_requested") is (event["location_services_enabled"] and event["state"] == "not_determined"),
                        "invalid permission request state")
                authorization_event = event
            elif kind in {"scan_started", "scan_observation"}:
                require(hello["command"] == "scan" and capabilities is not None
                        and capabilities["nearby_scan"]["state"] == "available", "scan without permission/capability")
                scan_id = event.get("scan_id")
                require(isinstance(scan_id, str) and UUID.fullmatch(scan_id) is not None, "scan UUID required")
                if kind == "scan_started":
                    require(scan_id not in scans and event.get("source_id") in sources, "duplicate scan or unknown source")
                    require(sources[event["source_id"]]["power_on"], "scan source is powered off")
                    start = uint64(event.get("api_started_monotonic_ns"))
                    require(start <= now and event.get("include_hidden") is False, "invalid scan start")
                    scans[scan_id] = (event["source_id"], start)
                else:
                    require(scan_id in scans, "observation without scan window")
                    identifier = event.get("observation_id")
                    require(isinstance(identifier, str) and UUID.fullmatch(identifier) is not None
                            and identifier not in observations, "invalid/duplicate observation")
                    observations.add(identifier)
                    require(len(observations) <= hello["max_observations"], "observation limit exceeded")
                    source_id = source(event.get("source"), session, hello)
                    require(source_id == scans[scan_id][0], "observation source mismatch")
                    window = event.get("api_window")
                    require(isinstance(window, dict), "API window required")
                    start, end = uint64(window.get("start_monotonic_ns")), uint64(window.get("end_monotonic_ns"))
                    require(start == scans[scan_id][1] and start <= end <= now, "invalid API window")
                    require(event.get("evidence_class") == "observed_api_result", "incorrect evidence class")
                    bssid = evidence(event.get("bssid"), bssid_value)
                    ssid = evidence(event.get("ssid_octets_base64"), lambda s: isinstance(s, str) and len(s) <= 44)
                    if ssid is not None:
                        try:
                            require(0 < len(base64.b64decode(ssid, validate=True)) <= 32, "invalid SSID bytes")
                        except ValueError as error:
                            raise ContractError("invalid SSID encoding") from error
                    if hello["identifier_policy"] == "redacted":
                        require(bssid is None and ssid is None and event["bssid"]["reason"] == "redacted"
                                and event["ssid_octets_base64"]["reason"] == "redacted", "identifier privacy violation")
                    channel(event.get("channel"))
                    for key in ["rssi_dbm", "noise_dbm"]:
                        evidence(event.get(key), lambda n: integer(n, -200, -1))
                    for key in ["result_age_seconds", "dwell_seconds", "phy", "position", "information_elements"]:
                        evidence(event.get(key))
                    require(event.get("measurement_method") == "CoreWLAN.CWNetwork.rssiValue/noiseMeasurement"
                            and event.get("calibration") == "uncalibrated", "signal provenance missing")
                    require(event.get("quality") == ["capture_time_unknown", "scan_cache_age_unknown", "uncalibrated", "dwell_unknown"],
                            "missing quality caveats")
            elif kind == "complete":
                status = event.get("status")
                require(isinstance(status, str) and status in STATUSES and text(event.get("reason")), "invalid completion")
                require(type(event.get("observation_count")) is int and event["observation_count"] == len(observations), "observation count mismatch")
                require(event.get("partial") is (status != "ok" and len(observations) > 0), "partial status mismatch")
                if status == "ok":
                    require(capabilities is not None or hello["command"] == "authorize", "success before capabilities")
                    if hello["command"] == "scan":
                        require(scans and capabilities["nearby_scan"]["state"] == "available", "scan success without scan")
                    if hello["command"] == "authorize":
                        require(authorization_event is not None and authorization_event["location_services_enabled"]
                                and event["reason"] == "location_authorized", "authorization success without consent")
                        require(authorization_event["state"] == "authorized" or authorization_event["prompt_requested"],
                                "authorization success from nonrequestable initial state")
                        require(event.get("final_authorization") == "authorized"
                                and event.get("final_location_services_enabled") is True,
                                "authorization success without final permission evidence")
                complete = event
            else:
                raise ContractError("unknown event kind")
        events.append(event)
    require(complete is not None, "incomplete process stream")
    return events


def decode_stream(data):
    """Decode with uniform typed failures for malformed external payload shapes."""
    try:
        return _decode_stream(data)
    except ContractError:
        raise
    except (TypeError, KeyError, AttributeError, OverflowError, ValueError, RecursionError) as error:
        raise ContractError("malformed external record") from error
