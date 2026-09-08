#!/usr/bin/env python3
"""Read Kyberia observation chunks with an independent PyArrow implementation.

This deliberately does not import Kyberia code. It checks the standard Parquet
reader's view of the row count, fixed schema metadata, and physical codec, and
compares the complete schemas for every supplied chunk.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


EXPECTED_METADATA_PREFIX = (
    "version=2;schema=kyberia-envelope-v2-fixed-superset-1;"
    "encoding=typed-column-paths;"
)
METADATA_KEY = b"kyberia_observation_chunk"
MIN_PYARROW_MAJOR = 15


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("artifacts", nargs="+", type=Path)
    parser.add_argument("--expected-rows", type=int, required=True)
    args = parser.parse_args()

    try:
        import pyarrow as pa
        import pyarrow.parquet as pq
    except ImportError as error:
        print(f"PyArrow unavailable: {error}", file=sys.stderr)
        return 2
    major = int(pa.__version__.split(".", maxsplit=1)[0])
    if major < MIN_PYARROW_MAJOR:
        raise RuntimeError(
            f"PyArrow {pa.__version__} is outside the supported range "
            f">={MIN_PYARROW_MAJOR}"
        )

    schemas = []
    reports = []
    for artifact in args.artifacts:
        parquet_file = pq.ParquetFile(artifact)
        metadata = parquet_file.schema_arrow.metadata or {}
        marker = metadata.get(METADATA_KEY, b"").decode("utf-8")
        if not marker.startswith(EXPECTED_METADATA_PREFIX):
            raise RuntimeError(f"{artifact}: missing fixed V2 schema marker")
        if parquet_file.metadata.num_rows != args.expected_rows:
            raise RuntimeError(
                f"{artifact}: expected {args.expected_rows} rows, "
                f"got {parquet_file.metadata.num_rows}"
            )
        for row_group_index in range(parquet_file.metadata.num_row_groups):
            row_group = parquet_file.metadata.row_group(row_group_index)
            for column_index in range(row_group.num_columns):
                codec = row_group.column(column_index).compression
                if codec != "UNCOMPRESSED":
                    raise RuntimeError(
                        f"{artifact}: row group {row_group_index} column "
                        f"{column_index} uses {codec}"
                    )
        table = parquet_file.read()
        if table.num_rows != args.expected_rows:
            raise RuntimeError(f"{artifact}: table row count differs from footer")
        schemas.append(parquet_file.schema_arrow)
        reports.append(
            {
                "path": str(artifact),
                "rows": table.num_rows,
                "columns": table.num_columns,
                "schema_fields": len(parquet_file.schema_arrow),
            }
        )

    first = schemas[0]
    if any(not first.equals(schema, check_metadata=True) for schema in schemas[1:]):
        raise RuntimeError("supplied chunks expose different PyArrow schemas")

    print(
        json.dumps(
            {
                "pyarrow_version": pa.__version__,
                "pyarrow_supported_range": f">={MIN_PYARROW_MAJOR}",
                "schema_equal": True,
                "artifacts": reports,
            },
            sort_keys=True,
        )
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, ValueError) as error:
        print(f"PyArrow oracle failed: {error}", file=sys.stderr)
        raise SystemExit(1)
