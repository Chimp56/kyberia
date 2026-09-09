#!/usr/bin/env python3
"""Bounded, neutral OpenRFPlan V1 interchange proof.

This module is an independent research boundary.  It intentionally imports no
Deconflict, RF Atlas, database, or renderer package.  The wire format is
canonical JSON so the fixture can be checked by another implementation without
sharing Python objects with this harness.
"""

from __future__ import annotations

import argparse
from decimal import Decimal, InvalidOperation
import hashlib
import json
import math
import re
import sys
from pathlib import Path
from typing import Any, Mapping


SCHEMA_NAME = "openrfplan"
SCHEMA_VERSION = 1
MAX_DOCUMENT_BYTES = 256 * 1024
MAX_NESTING = 16
MAX_NODES = 20_000
MAX_ARRAY_ITEMS = 4_096
MAX_OBJECT_KEYS = 128
MAX_EXTENSION_NAMESPACES = 32
MAX_STRING_BYTES = 4_096
MAX_ID_BYTES = 128
MAX_COORDINATE_ABS_M = 10_000_000.0
MAX_VERTICES = 4_096
MAX_SEED = (1 << 64) - 1
MAX_DECIMAL_EXPONENT = 1_024
MAX_DECIMAL_DIGITS = 4_096

_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$")
_EXTENSION = re.compile(r"^[a-z][a-z0-9_.-]{1,63}$")
_SHA256 = re.compile(r"^[0-9a-f]{64}$")
_UNKNOWN_REASONS = {"not_provided", "unsupported", "unparseable", "redacted"}
_SOURCE_KINDS = {"original_fixture", "user_supplied", "external_adapter"}
_GEOMETRY_KINDS = {"boundary", "wall", "slab", "opening"}
_CONSTRAINT_KINDS = {"channel_assignment", "max_tx_power"}
_OPERATORS = {"at_least", "at_most"}
_UNITS = {
    "length": "m",
    "frequency": "MHz",
    "power": "dBm",
    "angle": "deg",
    "weight": "unitless",
}


class InterchangeError(ValueError):
    """A stable, non-sensitive validation error for the proof boundary."""


class ResourceLimitError(InterchangeError):
    """Input exceeded a fixed parser or document budget."""


class DuplicateKeyError(InterchangeError):
    """JSON object contained a duplicate key."""


class UnsupportedVersionError(InterchangeError):
    """The producer selected a version this proof does not implement."""


class ReferenceError(InterchangeError):
    """A typed ID reference does not resolve within the document."""


def _fail(message: str, error_type: type[InterchangeError] = InterchangeError) -> None:
    raise error_type(message)


def _expect_mapping(value: Any, label: str) -> Mapping[str, Any]:
    if not isinstance(value, dict):
        _fail(f"{label} must be an object")
    return value


def _expect_keys(value: Mapping[str, Any], expected: set[str], label: str) -> None:
    actual = set(value)
    if actual != expected:
        missing = sorted(expected - actual)
        extra = sorted(actual - expected)
        detail = []
        if missing:
            detail.append("missing " + ",".join(missing))
        if extra:
            detail.append("unknown " + ",".join(extra))
        _fail(f"{label} has invalid fields ({'; '.join(detail)})")


def _string(value: Any, label: str, *, max_bytes: int = MAX_STRING_BYTES) -> str:
    if not isinstance(value, str):
        _fail(f"{label} must be a string")
    if len(value.encode("utf-8")) > max_bytes:
        _fail(f"{label} exceeds {max_bytes} UTF-8 bytes", ResourceLimitError)
    if any(ord(char) < 0x20 for char in value):
        _fail(f"{label} contains a control character")
    return value


def _id(value: Any, label: str) -> str:
    value = _string(value, label, max_bytes=MAX_ID_BYTES)
    if not _ID.fullmatch(value):
        _fail(f"{label} is not a bounded stable ID")
    return value


def _finite(value: Any, label: str, *, integer: bool = False) -> int | float | Decimal:
    if isinstance(value, bool) or not isinstance(value, (int, float, Decimal)):
        _fail(f"{label} must be a number")
    if isinstance(value, float) and not math.isfinite(value):
        _fail(f"{label} must be finite")
    if isinstance(value, Decimal) and not value.is_finite():
        _fail(f"{label} must be finite")
    if isinstance(value, Decimal) and abs(value.as_tuple().exponent) > MAX_DECIMAL_EXPONENT:
        _fail(f"{label} exceeds the bounded decimal exponent", ResourceLimitError)
    if isinstance(value, Decimal) and len(value.as_tuple().digits) > MAX_DECIMAL_DIGITS:
        _fail(f"{label} exceeds the bounded decimal coefficient", ResourceLimitError)
    if integer and not isinstance(value, int):
        _fail(f"{label} must be an integer")
    return value


def _bounded_array(value: Any, label: str, *, minimum: int = 0, maximum: int = MAX_ARRAY_ITEMS) -> list[Any]:
    if not isinstance(value, list):
        _fail(f"{label} must be an array")
    if not minimum <= len(value) <= maximum:
        _fail(f"{label} has {len(value)} items; allowed range is {minimum}..{maximum}", ResourceLimitError)
    return value


def _coordinate(value: Any, label: str) -> list[float | int]:
    values = _bounded_array(value, label, minimum=3, maximum=3)
    result = []
    for index, item in enumerate(values):
        number = _finite(item, f"{label}[{index}]")
        if not -MAX_COORDINATE_ABS_M <= number <= MAX_COORDINATE_ABS_M:
            _fail(f"{label}[{index}] is outside the bounded plan coordinate range")
        result.append(number)
    return result


def _typed(value: Any, label: str, unit: str, kind: str = "number", *, integer: bool = False) -> Any:
    value = _expect_mapping(value, label)
    if value.get("status") == "known":
        _expect_keys(value, {"status", "unit", "value"}, label)
        if value["unit"] != unit:
            _fail(f"{label} declares unit {value['unit']!r}, expected {unit!r}")
        if kind == "number":
            return _finite(value["value"], f"{label}.value", integer=integer)
        if kind == "string":
            return _string(value["value"], f"{label}.value")
        _fail(f"{label} uses an unsupported typed value kind")
    if value.get("status") == "unknown":
        _expect_keys(value, {"status", "unit", "reason", "raw"}, label)
        if value["unit"] != unit:
            _fail(f"{label} declares unit {value['unit']!r}, expected {unit!r}")
        if value["reason"] not in _UNKNOWN_REASONS:
            _fail(f"{label}.reason is not a supported explicit unknown reason")
        _string(value["raw"], f"{label}.raw", max_bytes=512)
        return None
    _fail(f"{label}.status must be known or unknown")


def _typed_channel(value: Any, label: str) -> int | None:
    result = _typed(value, label, "channel", integer=True)
    if result is not None and not 1 <= result <= 1000:
        _fail(f"{label}.value is outside the channel-number bound")
    return result


def _typed_width(value: Any, label: str) -> int | None:
    result = _typed(value, label, "MHz", integer=True)
    if result is not None and result not in {5, 10, 20, 40, 80, 160, 320}:
        _fail(f"{label}.value is not a supported planning width")
    return result


def _walk_budget(
    value: Any,
    *,
    depth: int = 0,
    state: list[int] | None = None,
    active: set[int] | None = None,
) -> None:
    """Bound every parsed JSON value, including namespaced extension values."""
    if state is None:
        state = [0]
    if active is None:
        active = set()
    state[0] += 1
    if state[0] > MAX_NODES:
        _fail("document contains too many JSON values", ResourceLimitError)
    if depth > MAX_NESTING:
        _fail("document nesting exceeds the interchange limit", ResourceLimitError)
    if isinstance(value, str):
        _string(value, "JSON string")
    elif isinstance(value, dict):
        identity = id(value)
        if identity in active:
            _fail("document contains a cyclic object")
        active.add(identity)
        if len(value) > MAX_OBJECT_KEYS:
            _fail("JSON object contains too many keys", ResourceLimitError)
        for key, child in value.items():
            _string(key, "JSON object key", max_bytes=MAX_STRING_BYTES)
            _walk_budget(child, depth=depth + 1, state=state, active=active)
        active.remove(identity)
    elif isinstance(value, list):
        identity = id(value)
        if identity in active:
            _fail("document contains a cyclic array")
        active.add(identity)
        if len(value) > MAX_ARRAY_ITEMS:
            _fail("JSON array contains too many items", ResourceLimitError)
        for child in value:
            _walk_budget(child, depth=depth + 1, state=state, active=active)
        active.remove(identity)
    elif isinstance(value, float) and not math.isfinite(value):
        _fail("JSON number must be finite")
    elif isinstance(value, Decimal):
        if not value.is_finite():
            _fail("JSON number must be finite")
        if abs(value.as_tuple().exponent) > MAX_DECIMAL_EXPONENT:
            _fail("JSON number exceeds the bounded decimal exponent", ResourceLimitError)
        if len(value.as_tuple().digits) > MAX_DECIMAL_DIGITS:
            _fail("JSON number exceeds the bounded decimal coefficient", ResourceLimitError)
    elif isinstance(value, int) and value.bit_length() > 64:
        _fail("JSON integer exceeds the 64-bit planning bound", ResourceLimitError)
    elif not isinstance(value, (type(None), bool, int, float)):
        _fail("JSON contains an unsupported value")


def _preflight_json(raw: bytes) -> str:
    if len(raw) > MAX_DOCUMENT_BYTES:
        _fail(f"document exceeds {MAX_DOCUMENT_BYTES} bytes", ResourceLimitError)
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError as error:
        _fail(f"document is not UTF-8: {error}")
    depth = 0
    in_string = False
    escaped = False
    for character in text:
        if in_string:
            if escaped:
                escaped = False
            elif character == "\\":
                escaped = True
            elif character == '"':
                in_string = False
            elif ord(character) < 0x20:
                _fail("JSON string contains a raw control character")
            continue
        if character == '"':
            in_string = True
        elif character in "[{":
            depth += 1
            if depth > MAX_NESTING:
                _fail("JSON nesting exceeds the interchange limit", ResourceLimitError)
        elif character in "]}":
            depth -= 1
            if depth < 0:
                _fail("JSON closes more containers than it opens")
    if in_string or escaped or depth != 0:
        _fail("JSON has an unterminated string or container")
    return text


def _object_pairs(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            _fail(f"duplicate JSON key {key!r}", DuplicateKeyError)
        result[key] = value
    return result


def _parse_int(token: str) -> int:
    if len(token.lstrip("-")) > 20:
        _fail("integer token exceeds the 64-bit planning bound", ResourceLimitError)
    return int(token)


def _parse_float(token: str) -> Decimal:
    try:
        value = Decimal(token)
    except InvalidOperation:
        _fail("JSON number is not a valid decimal")
    if not value.is_finite():
        _fail("JSON number must be finite")
    if abs(value.as_tuple().exponent) > MAX_DECIMAL_EXPONENT:
        _fail("JSON number exceeds the bounded decimal exponent", ResourceLimitError)
    if len(value.as_tuple().digits) > MAX_DECIMAL_DIGITS:
        _fail("JSON number exceeds the bounded decimal coefficient", ResourceLimitError)
    return value


def _parse_constant(token: str) -> None:
    _fail(f"non-standard JSON constant {token!r} is not allowed")


def _ref(value: Any, known: set[str], label: str) -> str:
    value = _id(value, label)
    if value not in known:
        _fail(f"{label} {value!r} does not resolve", ReferenceError)
    return value


def _unique_ids(items: list[Any], label: str, ids: set[str]) -> None:
    for index, item in enumerate(items):
        item = _expect_mapping(item, f"{label}[{index}]")
        item_id = _id(item.get("id"), f"{label}[{index}].id")
        if item_id in ids:
            _fail(f"duplicate document ID {item_id!r}")
        ids.add(item_id)


def _validate_document(document: Mapping[str, Any]) -> None:
    top = _expect_mapping(document, "document")
    _expect_keys(top, {
        "schema", "plan_id", "units", "coordinate_frames", "floors", "materials",
        "geometry", "access_points", "radios", "channel_constraints", "zones",
        "requirements", "seeds", "provenance", "extensions",
    }, "document")

    schema = _expect_mapping(top["schema"], "schema")
    _expect_keys(schema, {"name", "version"}, "schema")
    if schema["name"] != SCHEMA_NAME:
        _fail(f"unsupported schema name {schema['name']!r}", UnsupportedVersionError)
    if type(schema["version"]) is not int or schema["version"] != SCHEMA_VERSION:
        _fail(f"unsupported {SCHEMA_NAME} version {schema['version']!r}", UnsupportedVersionError)
    plan_id = _id(top["plan_id"], "plan_id")

    units = _expect_mapping(top["units"], "units")
    _expect_keys(units, set(_UNITS), "units")
    if dict(units) != _UNITS:
        _fail("document units do not match the V1 physical contract")

    frames = _bounded_array(top["coordinate_frames"], "coordinate_frames", minimum=1, maximum=64)
    floors = _bounded_array(top["floors"], "floors", minimum=1, maximum=256)
    materials = _bounded_array(top["materials"], "materials", maximum=1024)
    geometry = _bounded_array(top["geometry"], "geometry", maximum=4096)
    access_points = _bounded_array(top["access_points"], "access_points", maximum=4096)
    radios = _bounded_array(top["radios"], "radios", maximum=4096)
    constraints = _bounded_array(top["channel_constraints"], "channel_constraints", maximum=4096)
    zones = _bounded_array(top["zones"], "zones", maximum=4096)
    requirements = _bounded_array(top["requirements"], "requirements", maximum=4096)
    seeds = _bounded_array(top["seeds"], "seeds", minimum=1, maximum=64)
    provenance = _expect_mapping(top["provenance"], "provenance")
    extensions = _expect_mapping(top["extensions"], "extensions")
    if len(extensions) > MAX_EXTENSION_NAMESPACES:
        _fail("too many extension namespaces", ResourceLimitError)
    for namespace, extension in extensions.items():
        if not _EXTENSION.fullmatch(namespace):
            _fail(f"extension namespace {namespace!r} is not namespaced")
        _walk_budget(extension)

    ids: set[str] = {plan_id}
    for items, label in [
        (frames, "coordinate_frames"), (floors, "floors"), (materials, "materials"),
        (geometry, "geometry"), (access_points, "access_points"), (radios, "radios"),
        (constraints, "channel_constraints"), (zones, "zones"),
        (requirements, "requirements"), (seeds, "seeds"),
    ]:
        _unique_ids(items, label, ids)

    frame_ids = {item["id"] for item in frames}
    floor_ids = {item["id"] for item in floors}
    material_ids = {item["id"] for item in materials}
    geometry_ids = {item["id"] for item in geometry}
    ap_ids = {item["id"] for item in access_points}
    radio_ids = {item["id"] for item in radios}
    zone_ids = {item["id"] for item in zones}
    seed_ids = {item["id"] for item in seeds}

    for index, frame in enumerate(frames):
        frame = _expect_mapping(frame, f"coordinate_frames[{index}]")
        _expect_keys(frame, {"id", "parent_id", "handedness", "axis_order", "origin_m", "orientation_deg"}, f"coordinate_frames[{index}]")
        _id(frame["id"], f"coordinate_frames[{index}].id")
        if frame["parent_id"] is not None:
            _ref(frame["parent_id"], frame_ids, f"coordinate_frames[{index}].parent_id")
        if frame["handedness"] != "right":
            _fail("V1 requires an explicit right-handed coordinate frame")
        if frame["axis_order"] != ["x", "y", "z"]:
            _fail("V1 requires x,y,z axis order")
        _coordinate(frame["origin_m"], f"coordinate_frames[{index}].origin_m")
        orientation = _coordinate(frame["orientation_deg"], f"coordinate_frames[{index}].orientation_deg")
        if any(not -360 <= item <= 360 for item in orientation):
            _fail("coordinate frame orientation exceeds the bounded range")
        if frame["parent_id"] is None and any(item != 0 for item in orientation):
            _fail("the world-root frame must use identity orientation")

    # A parent cycle would make a frame reference ambiguous even though every
    # individual ID resolves, so detect it separately from shape validation.
    parents = {frame["id"]: frame["parent_id"] for frame in frames}
    for frame_id in parents:
        seen: set[str] = set()
        cursor: str | None = frame_id
        while cursor is not None:
            if cursor in seen:
                _fail("coordinate frame parent cycle")
            seen.add(cursor)
            cursor = parents[cursor]

    for index, floor in enumerate(floors):
        floor = _expect_mapping(floor, f"floors[{index}]")
        _expect_keys(floor, {"id", "frame_id", "elevation_m"}, f"floors[{index}]")
        _id(floor["id"], f"floors[{index}].id")
        _ref(floor["frame_id"], frame_ids, f"floors[{index}].frame_id")
        _typed(floor["elevation_m"], f"floors[{index}].elevation_m", "m")

    source_ids: set[str] = set()
    _expect_keys(provenance, {"format", "producer", "producer_version", "sources", "seed_id"}, "provenance")
    if provenance["format"] != "openrfplan-canonical-json":
        _fail("provenance format does not identify the V1 canonical encoding")
    _string(provenance["producer"], "provenance.producer")
    _string(provenance["producer_version"], "provenance.producer_version")
    sources = _bounded_array(provenance["sources"], "provenance.sources", minimum=1, maximum=64)
    for index, source in enumerate(sources):
        source = _expect_mapping(source, f"provenance.sources[{index}]")
        _expect_keys(source, {"id", "kind", "uri", "sha256", "license"}, f"provenance.sources[{index}]")
        source_id = _id(source["id"], f"provenance.sources[{index}].id")
        if source_id in source_ids:
            _fail(f"duplicate provenance source ID {source_id!r}")
        source_ids.add(source_id)
        if source["kind"] not in _SOURCE_KINDS:
            _fail("unsupported provenance source kind")
        _string(source["uri"], f"provenance.sources[{index}].uri")
        if not isinstance(source["sha256"], str) or not _SHA256.fullmatch(source["sha256"]):
            _fail("provenance source hash must be lowercase SHA-256")
        _string(source["license"], f"provenance.sources[{index}].license", max_bytes=256)
    _ref(provenance["seed_id"], seed_ids, "provenance.seed_id")

    for index, material in enumerate(materials):
        material = _expect_mapping(material, f"materials[{index}]")
        _expect_keys(material, {"id", "label", "catalog_ref"}, f"materials[{index}]")
        _id(material["id"], f"materials[{index}].id")
        _string(material["label"], f"materials[{index}].label", max_bytes=256)
        catalog = _expect_mapping(material["catalog_ref"], f"materials[{index}].catalog_ref")
        _expect_keys(catalog, {"id", "source_id", "version"}, f"materials[{index}].catalog_ref")
        _id(catalog["id"], f"materials[{index}].catalog_ref.id")
        _ref(catalog["source_id"], source_ids, f"materials[{index}].catalog_ref.source_id")
        _string(catalog["version"], f"materials[{index}].catalog_ref.version", max_bytes=128)

    for index, item in enumerate(geometry):
        item = _expect_mapping(item, f"geometry[{index}]")
        _expect_keys(item, {"id", "kind", "floor_id", "frame_id", "material_id", "closed", "vertices_m"}, f"geometry[{index}]")
        _id(item["id"], f"geometry[{index}].id")
        if item["kind"] not in _GEOMETRY_KINDS:
            _fail(f"geometry[{index}].kind is unsupported")
        _ref(item["floor_id"], floor_ids, f"geometry[{index}].floor_id")
        _ref(item["frame_id"], frame_ids, f"geometry[{index}].frame_id")
        if item["material_id"] is not None:
            _ref(item["material_id"], material_ids, f"geometry[{index}].material_id")
        if not isinstance(item["closed"], bool):
            _fail(f"geometry[{index}].closed must be boolean")
        if item["kind"] in {"boundary", "slab"} and not item["closed"]:
            _fail(f"geometry[{index}] must close a boundary or slab")
        if item["kind"] in {"wall", "opening"} and item["closed"]:
            _fail(f"geometry[{index}] cannot close a wall or opening")
        vertices = _bounded_array(item["vertices_m"], f"geometry[{index}].vertices_m", minimum=2, maximum=MAX_VERTICES)
        if item["kind"] in {"boundary", "slab"} and len(vertices) < 3:
            _fail(f"geometry[{index}] needs at least three vertices")
        for vertex_index, vertex in enumerate(vertices):
            _coordinate(vertex, f"geometry[{index}].vertices_m[{vertex_index}]")

    ap_radio_ids: set[str] = set()
    for index, ap in enumerate(access_points):
        ap = _expect_mapping(ap, f"access_points[{index}]")
        _expect_keys(ap, {"id", "name", "floor_id", "frame_id", "position_m", "radio_ids"}, f"access_points[{index}]")
        _id(ap["id"], f"access_points[{index}].id")
        _string(ap["name"], f"access_points[{index}].name", max_bytes=256)
        _ref(ap["floor_id"], floor_ids, f"access_points[{index}].floor_id")
        _ref(ap["frame_id"], frame_ids, f"access_points[{index}].frame_id")
        _coordinate(ap["position_m"], f"access_points[{index}].position_m")
        listed = _bounded_array(ap["radio_ids"], f"access_points[{index}].radio_ids", maximum=64)
        for radio_index, radio_id in enumerate(listed):
            radio_id = _ref(radio_id, radio_ids, f"access_points[{index}].radio_ids[{radio_index}]")
            if radio_id in ap_radio_ids:
                _fail(f"radio {radio_id!r} is assigned to more than one access point")
            ap_radio_ids.add(radio_id)

    ap_by_id = {item["id"]: item for item in access_points}
    for index, radio in enumerate(radios):
        radio = _expect_mapping(radio, f"radios[{index}]")
        _expect_keys(radio, {"id", "access_point_id", "floor_id", "frame_id", "position_m", "channel", "tx_power_dbm"}, f"radios[{index}]")
        radio_id = _id(radio["id"], f"radios[{index}].id")
        access_point_id = _ref(radio["access_point_id"], ap_ids, f"radios[{index}].access_point_id")
        _ref(radio["floor_id"], floor_ids, f"radios[{index}].floor_id")
        _ref(radio["frame_id"], frame_ids, f"radios[{index}].frame_id")
        access_point = ap_by_id[access_point_id]
        if radio["floor_id"] != access_point["floor_id"] or radio["frame_id"] != access_point["frame_id"]:
            _fail(f"radios[{index}] must use its access point's floor and frame")
        _coordinate(radio["position_m"], f"radios[{index}].position_m")
        channel = _expect_mapping(radio["channel"], f"radios[{index}].channel")
        _expect_keys(channel, {"band", "number", "width_mhz"}, f"radios[{index}].channel")
        _typed(channel["band"], f"radios[{index}].channel.band", "band", "string")
        _typed_channel(channel["number"], f"radios[{index}].channel.number")
        _typed_width(channel["width_mhz"], f"radios[{index}].channel.width_mhz")
        _typed(radio["tx_power_dbm"], f"radios[{index}].tx_power_dbm", "dBm")
        if radio_id not in ap_radio_ids:
            _fail(f"radio {radio_id!r} is not listed by its access point")

    for index, constraint in enumerate(constraints):
        constraint = _expect_mapping(constraint, f"channel_constraints[{index}]")
        _expect_keys(constraint, {"id", "radio_id", "kind", "allowed_channels", "max_tx_power_dbm"}, f"channel_constraints[{index}]")
        _id(constraint["id"], f"channel_constraints[{index}].id")
        _ref(constraint["radio_id"], radio_ids, f"channel_constraints[{index}].radio_id")
        if constraint["kind"] not in _CONSTRAINT_KINDS:
            _fail(f"channel_constraints[{index}].kind is unsupported")
        channels = _bounded_array(constraint["allowed_channels"], f"channel_constraints[{index}].allowed_channels", maximum=64)
        if constraint["kind"] == "channel_assignment" and not channels:
            _fail(f"channel_constraints[{index}] needs at least one allowed channel")
        channel_values: set[tuple[Any, ...]] = set()
        for channel_index, channel in enumerate(channels):
            channel_label = f"channel_constraints[{index}].allowed_channels[{channel_index}]"
            known_channel = _typed_channel(channel, channel_label)
            if known_channel is None:
                channel_key = ("unknown", channel["reason"], channel["raw"])
            else:
                channel_key = ("known", known_channel)
            if channel_key in channel_values:
                _fail(f"{channel_label} duplicates an allowed channel")
            channel_values.add(channel_key)
        _typed(constraint["max_tx_power_dbm"], f"channel_constraints[{index}].max_tx_power_dbm", "dBm")

    for index, zone in enumerate(zones):
        zone = _expect_mapping(zone, f"zones[{index}]")
        _expect_keys(zone, {"id", "floor_id", "frame_id", "boundary_geometry_id", "demand_weight"}, f"zones[{index}]")
        _id(zone["id"], f"zones[{index}].id")
        _ref(zone["floor_id"], floor_ids, f"zones[{index}].floor_id")
        _ref(zone["frame_id"], frame_ids, f"zones[{index}].frame_id")
        boundary_id = _ref(zone["boundary_geometry_id"], geometry_ids, f"zones[{index}].boundary_geometry_id")
        boundary = next(item for item in geometry if item["id"] == boundary_id)
        if boundary["kind"] != "boundary":
            _fail(f"zones[{index}] must refer to a boundary geometry")
        if zone["floor_id"] != boundary["floor_id"] or zone["frame_id"] != boundary["frame_id"]:
            _fail(f"zones[{index}] must use its boundary geometry's floor and frame")
        _typed(zone["demand_weight"], f"zones[{index}].demand_weight", "unitless")

    for index, requirement in enumerate(requirements):
        requirement = _expect_mapping(requirement, f"requirements[{index}]")
        _expect_keys(requirement, {"id", "zone_id", "metric_id", "operator", "threshold"}, f"requirements[{index}]")
        _id(requirement["id"], f"requirements[{index}].id")
        _ref(requirement["zone_id"], zone_ids, f"requirements[{index}].zone_id")
        _id(requirement["metric_id"], f"requirements[{index}].metric_id")
        if requirement["operator"] not in _OPERATORS:
            _fail(f"requirements[{index}].operator is unsupported")
        _typed(requirement["threshold"], f"requirements[{index}].threshold", "dBm")

    for index, seed in enumerate(seeds):
        seed = _expect_mapping(seed, f"seeds[{index}]")
        _expect_keys(seed, {"id", "algorithm", "value"}, f"seeds[{index}]")
        _id(seed["id"], f"seeds[{index}].id")
        _string(seed["algorithm"], f"seeds[{index}].algorithm", max_bytes=128)
        value = _finite(seed["value"], f"seeds[{index}].value", integer=True)
        if not 0 <= value <= MAX_SEED:
            _fail(f"seeds[{index}].value exceeds the unsigned 64-bit bound")


def _canonical_number(value: int | float | Decimal) -> str:
    """Emit the V1 decimal grammar without exponent or representation aliases."""
    if isinstance(value, bool):
        _fail("boolean is not a number")
    if isinstance(value, int):
        return str(value)
    if isinstance(value, float):
        if not math.isfinite(value):
            _fail("JSON number must be finite")
        value = Decimal(str(value))
    if not isinstance(value, Decimal) or not value.is_finite():
        _fail("JSON number must be finite")
    if abs(value.as_tuple().exponent) > MAX_DECIMAL_EXPONENT:
        _fail("JSON number exceeds the bounded decimal exponent", ResourceLimitError)
    if value == 0:
        return "0"
    rendered = format(value, "f")
    if "." in rendered:
        rendered = rendered.rstrip("0").rstrip(".")
    if rendered in {"", "-0"}:
        return "0"
    return rendered


def _canonical_json(value: Any) -> str:
    """Serialize the already validated value using the V1 canonical grammar."""
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, (int, float, Decimal)):
        return _canonical_number(value)
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=True, separators=(",", ":"))
    if isinstance(value, list):
        return "[" + ",".join(_canonical_json(item) for item in value) + "]"
    if isinstance(value, dict):
        return "{" + ",".join(
            _canonical_json(key) + ":" + _canonical_json(value[key])
            for key in sorted(value)
        ) + "}"
    _fail("JSON contains an unsupported value")


def _normalize(value: Any) -> Any:
    if isinstance(value, dict):
        return {key: _normalize(value[key]) for key in sorted(value)}
    if isinstance(value, list):
        return [_normalize(item) for item in value]
    if isinstance(value, float) and value == 0:
        return 0.0
    return value


def _normalize_document(document: Mapping[str, Any]) -> dict[str, Any]:
    normalized = _normalize(dict(document))
    for key in [
        "coordinate_frames", "floors", "materials", "geometry", "access_points", "radios",
        "channel_constraints", "zones", "requirements", "seeds",
    ]:
        normalized[key] = sorted(normalized[key], key=lambda item: item["id"])
    for access_point in normalized["access_points"]:
        access_point["radio_ids"] = sorted(access_point["radio_ids"])
    for constraint in normalized["channel_constraints"]:
        constraint["allowed_channels"] = sorted(
            constraint["allowed_channels"], key=lambda item: (
                0, item["value"]
            ) if item["status"] == "known" else (
                1, item["reason"], item["raw"]
            )
        )
    normalized["provenance"]["sources"] = sorted(normalized["provenance"]["sources"], key=lambda item: item["id"])
    return normalized


def encode_document(document: Mapping[str, Any]) -> bytes:
    """Validate and emit deterministic UTF-8 canonical JSON bytes."""
    _walk_budget(document)
    _validate_document(document)
    normalized = _normalize_document(document)
    encoded = _canonical_json(normalized).encode("utf-8")
    if len(encoded) > MAX_DOCUMENT_BYTES:
        _fail("canonical document exceeds the interchange byte limit", ResourceLimitError)
    return encoded


def decode_document(raw: bytes | str) -> dict[str, Any]:
    """Parse a bounded document and retain explicit unknown values verbatim."""
    if isinstance(raw, str):
        raw = raw.encode("utf-8")
    text = _preflight_json(raw)
    try:
        document = json.loads(
            text,
            object_pairs_hook=_object_pairs,
            parse_int=_parse_int,
            parse_float=_parse_float,
            parse_constant=_parse_constant,
        )
    except InterchangeError:
        raise
    except (json.JSONDecodeError, OverflowError, ValueError) as error:
        _fail(f"malformed JSON: {error}")
    if not isinstance(document, dict):
        _fail("top-level document must be an object")
    _walk_budget(document)
    _validate_document(document)
    return document


def decode_canonical_document(raw: bytes | str) -> dict[str, Any]:
    document = decode_document(raw)
    canonical = encode_document(document)
    original = raw.encode("utf-8") if isinstance(raw, str) else raw
    if canonical != original:
        _fail("document is valid but not canonical JSON")
    return document


def read_document(path: str | Path) -> tuple[dict[str, Any], bytes]:
    path = Path(path)
    try:
        with path.open("rb") as handle:
            raw = handle.read(MAX_DOCUMENT_BYTES + 1)
    except OSError as error:
        _fail(f"cannot read interchange input: {error}")
    if len(raw) > MAX_DOCUMENT_BYTES:
        _fail(f"document exceeds {MAX_DOCUMENT_BYTES} bytes", ResourceLimitError)
    return decode_document(raw), raw


def canonical_bytes(document: Mapping[str, Any]) -> bytes:
    return encode_document(document)


def canonical_sha256(document: Mapping[str, Any]) -> str:
    return hashlib.sha256(canonical_bytes(document)).hexdigest()


def summary(document: Mapping[str, Any]) -> dict[str, Any]:
    encoded = canonical_bytes(document)
    return {
        "schema": f"{SCHEMA_NAME}/{SCHEMA_VERSION}",
        "plan_id": document["plan_id"],
        "canonical_bytes": len(encoded),
        "canonical_sha256": hashlib.sha256(encoded).hexdigest(),
        "counts": {key: len(document[key]) for key in [
            "coordinate_frames", "floors", "materials", "geometry", "access_points",
            "radios", "channel_constraints", "zones", "requirements", "seeds",
        ]},
    }


def _cli_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="bounded OpenRFPlan V1 canonical JSON proof")
    subparsers = parser.add_subparsers(dest="command", required=True)
    for command in ("validate", "canonicalize"):
        subparser = subparsers.add_parser(command)
        subparser.add_argument("input", type=Path)
        if command == "canonicalize":
            subparser.add_argument("output", type=Path, nargs="?", default=Path("-"))
    return parser


def main(argv: list[str] | None = None) -> int:
    args = _cli_parser().parse_args(argv)
    try:
        document, _ = read_document(args.input)
        if args.command == "validate":
            sys.stdout.buffer.write((json.dumps(summary(document), sort_keys=True, separators=(",", ":")) + "\n").encode())
        else:
            output = canonical_bytes(document)
            if args.output == Path("-"):
                sys.stdout.buffer.write(output)
            else:
                args.output.write_bytes(output)
    except InterchangeError as error:
        print(f"interchange validation failed: {error}", file=sys.stderr)
        return 2
    except OSError as error:
        print(f"interchange output failed: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
