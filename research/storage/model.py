"""Original, bounded storage projection; not the canonical observation schema.

Positions are versioned assignments, not destructive changes to raw capture.
No foreign engine types cross this module. See docs/validation/storage-proof.md.
"""

from __future__ import annotations

from dataclasses import astuple, dataclass, fields
import hashlib
import json
import math

VERSION = 1
MAX_BATCH = 4096
MAX_ROWS = 1_000_000
BASE_NS = 1_783_000_000_000_000_007


@dataclass(frozen=True)
class Row:
    observation_id: bytes
    session_id: bytes
    source_id: bytes
    sensor_id: bytes
    adapter_id: bytes
    epoch_id: bytes
    utc_ns: int
    monotonic_ns: int
    floor_id: bytes
    frame_id: bytes
    x_m: float
    y_m: float
    z_m: float
    assignment_version: str
    frequency_hz: int
    rssi_dbm: float | None
    rssi_unknown: str | None
    noise_dbm: float | None
    noise_unknown: str | None
    source_version: str | None
    source_version_unknown: str | None
    adapter_version: str
    calibration_state: str
    pose_covariance_state: str
    raw_sha256: bytes
    quality: str

    def validate(self):
        for name in ID_FIELDS:
            value = getattr(self, name)
            if type(value) is not bytes or len(value) != 16 or not any(value):
                raise ValueError(f"invalid identity: {name}")
        for name in ("utc_ns", "monotonic_ns", "frequency_hz"):
            value = getattr(self, name)
            if type(value) is not int or not -(2**63) <= value < 2**63:
                raise ValueError(f"invalid signed integer projection: {name}")
        if self.monotonic_ns < 0 or self.frequency_hz <= 0:
            raise ValueError("invalid monotonic time/frequency")
        for name in ("x_m", "y_m", "z_m"):
            if type(getattr(self, name)) is not float or not math.isfinite(
                getattr(self, name)
            ):
                raise ValueError(f"invalid coordinate: {name}")
        for value_name, unknown_name in (
            ("rssi_dbm", "rssi_unknown"),
            ("noise_dbm", "noise_unknown"),
            ("source_version", "source_version_unknown"),
        ):
            value, reason = getattr(self, value_name), getattr(self, unknown_name)
            if (value is None) == (reason is None):
                raise ValueError(
                    "value must have exactly one known/unknown representation"
                )
            if reason is not None and reason not in (
                "not_observable",
                "not_reported",
                "redacted",
            ):
                raise ValueError("unsupported unknown reason")
            if (
                value is not None
                and value_name.endswith("dbm")
                and (type(value) is not float or not math.isfinite(value))
            ):
                raise ValueError("signal must be a finite dBm value")
        for name in TEXT_FIELDS:
            value = getattr(self, name)
            if value is None and name not in (
                "source_version",
                "source_version_unknown",
                "rssi_unknown",
                "noise_unknown",
            ):
                raise ValueError(f"missing required text: {name}")
            if value is not None and (
                type(value) is not str or not 0 < len(value.encode("utf8")) <= 128
            ):
                raise ValueError(f"invalid text: {name}")
        if (
            self.calibration_state != "uncalibrated"
            or self.pose_covariance_state != "not_measured"
            or self.quality != "synthetic_fixture"
        ):
            raise ValueError("unsupported research provenance state")
        if type(self.raw_sha256) is not bytes or len(self.raw_sha256) != 32:
            raise ValueError("invalid source hash")


NAMES = tuple(f.name for f in fields(Row))
ID_FIELDS = tuple(n for n in NAMES if n.endswith("_id"))
TEXT_FIELDS = tuple(
    n for n in NAMES if n.endswith(("_unknown", "_version", "_state")) or n == "quality"
)


def batch_validate(rows):
    if not 0 < len(rows) <= MAX_BATCH:
        raise ValueError("batch requires 1..4096 observations")
    seen = set()
    for row in rows:
        row.validate()
        if row.observation_id in seen:
            raise ValueError("duplicate observation identity")
        seen.add(row.observation_id)


def generate(count, seed=42, batch_size=MAX_BATCH):
    """LCG positions/signals, 4 floors/8 sources, 25 ms cadence (1M ≈ 6.94h).

    IDs encode sequence, not measured MACs. Repeat source hashes deliberately
    represent independent observations of identical raw evidence, not duplicates.
    """
    if (
        type(count) is not int
        or not 1 <= count <= MAX_ROWS
        or not 1 <= batch_size <= MAX_BATCH
    ):
        raise ValueError("invalid generation bounds")
    if type(seed) is not int or not 0 <= seed < 2**32:
        raise ValueError("seed must be uint32")

    def ident(n):
        return n.to_bytes(16, "big")

    batch, state = [], seed
    for i in range(count):
        state = (1664525 * state + 1013904223) % 2**32
        floor = i % 4
        row = Row(
            ident(i + 1),
            ident(100),
            ident(i % 8 + 200),
            ident(i % 8 + 300),
            ident(i % 8 + 400),
            ident(i % 8 + 500),
            BASE_NS + i * 25_000_001,
            i * 25_000_001,
            ident(floor + 600),
            ident(floor + 700),
            float(state % 10000) / 100,
            float((state >> 8) % 10000) / 100,
            float(floor * 3),
            "assignment/1",
            (2412000000, 5180000000, 5955000000)[i % 3],
            None if i % 7 == 0 else float(-30 - state % 71),
            "not_observable" if i % 7 == 0 else None,
            None if i % 3 == 0 else -95.0,
            "not_reported" if i % 3 == 0 else None,
            None if i % 5 == 0 else "fixture-source/1",
            "not_reported" if i % 5 == 0 else None,
            "storage-fixture/1",
            "uncalibrated",
            "not_measured",
            hashlib.sha256(f"original-packet-{i % 8192}".encode()).digest(),
            "synthetic_fixture",
        )
        batch.append(row)
        if len(batch) == batch_size:
            yield batch
            batch = []
    if batch:
        yield batch


def encode_row(row):
    return (
        json.dumps(
            [v.hex() if isinstance(v, bytes) else v for v in astuple(row)],
            ensure_ascii=True,
            allow_nan=False,
            separators=(",", ":"),
        )
        + "\n"
    ).encode()


def digest_rows(rows):
    digest, count = hashlib.sha256(), 0
    for row in rows:
        row.validate()
        digest.update(encode_row(row))
        count += 1
    return {"rows": count, "semantic_sha256": digest.hexdigest()}
