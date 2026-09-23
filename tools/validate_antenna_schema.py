#!/usr/bin/env python3
"""Validate the antenna-pattern v1 schema and structural acceptance cases."""

import copy
import importlib.metadata
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCHEMA_PATH = ROOT / "crates/antenna-model/schema/antenna-pattern-v1.schema.json"
VALID_FIXTURE_PATH = ROOT / "crates/antenna-model/tests/fixtures/antenna-pattern-v1-valid.json"
PINNED_JSONSCHEMA = "4.25.1"


def main():
    try:
        from jsonschema import Draft202012Validator
    except ModuleNotFoundError as error:
        if error.name == "jsonschema":
            print(
                "ERROR: jsonschema=={} is required; run python3 tools/dev.py supply-chain-bootstrap "
                "with the configured developer Python.".format(PINNED_JSONSCHEMA),
                file=sys.stderr,
            )
            return 2
        raise

    try:
        installed = importlib.metadata.version("jsonschema")
    except importlib.metadata.PackageNotFoundError:
        print("ERROR: the jsonschema package metadata is unavailable.", file=sys.stderr)
        return 2
    if installed != PINNED_JSONSCHEMA:
        print(
            "ERROR: expected pinned jsonschema {}, found {}.".format(
                PINNED_JSONSCHEMA, installed
            ),
            file=sys.stderr,
        )
        return 2

    try:
        schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
        valid_instance = json.loads(VALID_FIXTURE_PATH.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        print("ERROR: could not load schema or valid fixture: {}".format(error), file=sys.stderr)
        return 2

    Draft202012Validator.check_schema(schema)
    validator = Draft202012Validator(schema)
    validator.validate(valid_instance)

    optional_cross_plane = copy.deepcopy(valid_instance)
    for frequency in optional_cross_plane["frequencies"]:
        for sample in frequency["samples"]:
            sample.pop("cross_polar_gain_dbi")
    validator.validate(optional_cross_plane)

    invalid_instances = {}

    unknown_property = copy.deepcopy(valid_instance)
    unknown_property["unversioned_extra"] = True
    invalid_instances["closed top-level properties"] = unknown_property

    missing_coordinate_axis = copy.deepcopy(valid_instance)
    del missing_coordinate_axis["coordinate_convention"]["azimuth_zero_axis"]
    invalid_instances["required coordinate convention"] = missing_coordinate_axis

    missing_elevation_pole = copy.deepcopy(valid_instance)
    missing_elevation_pole["elevation_degrees"] = [-80.0, 0.0, 90.0]
    invalid_instances["required elevation endpoint"] = missing_elevation_pole

    malformed_gain = copy.deepcopy(valid_instance)
    malformed_gain["frequencies"][0]["samples"][4]["co_polar_gain_dbi"] = "6 dBi"
    invalid_instances["numeric gain sample"] = malformed_gain

    trailing_lf_model_id = copy.deepcopy(valid_instance)
    trailing_lf_model_id["model_id"] += "\n"
    invalid_instances["trailing line feed in model_id"] = trailing_lf_model_id

    trailing_lf_license = copy.deepcopy(valid_instance)
    trailing_lf_license["source"]["license_spdx"] += "\n"
    invalid_instances["trailing line feed in source.license_spdx"] = trailing_lf_license

    trailing_lf_uri = copy.deepcopy(valid_instance)
    trailing_lf_uri["source"]["source_uri"] += "\n"
    invalid_instances["trailing line feed in source.source_uri"] = trailing_lf_uri

    trailing_lf_checksum = copy.deepcopy(valid_instance)
    trailing_lf_checksum["source"]["source_checksum_sha256"] += "\n"
    invalid_instances[
        "trailing line feed in source.source_checksum_sha256"
    ] = trailing_lf_checksum

    for label, instance in invalid_instances.items():
        if validator.is_valid(instance):
            print("ERROR: invalid instance passed schema: {}".format(label), file=sys.stderr)
            return 1

    print(
        "PASS: Draft 2020-12 schema is valid; canonical Rust fixture and optional cross-plane "
        "form accepted; {} invalid-instance rejection cases passed (jsonschema {}).".format(
            len(invalid_instances), installed
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
