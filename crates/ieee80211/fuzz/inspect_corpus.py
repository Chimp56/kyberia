#!/usr/bin/env python3
"""Report a deterministic digest for one flat libFuzzer corpus directory."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path


ALGORITHM = "sha256(sorted UTF-8 basename + NUL + decimal size + NUL + content)"


def inspect(root: Path) -> dict[str, object]:
    if not root.is_dir():
        raise SystemExit(f"not a corpus directory: {root}")

    entries = sorted(root.iterdir(), key=lambda path: path.name)
    unexpected = [path.name for path in entries if not path.is_file() or path.is_symlink()]
    if unexpected:
        raise SystemExit(f"corpus must contain only regular files: {unexpected}")

    digest = hashlib.sha256()
    total_bytes = 0
    for path in entries:
        name = path.name.encode("utf-8", "strict")
        content = path.read_bytes()
        total_bytes += len(content)
        digest.update(name)
        digest.update(b"\0")
        digest.update(str(len(content)).encode("ascii"))
        digest.update(b"\0")
        digest.update(content)

    return {
        "schema_version": 1,
        "algorithm": ALGORITHM,
        "files": len(entries),
        "bytes": total_bytes,
        "sha256": digest.hexdigest(),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("corpus", type=Path)
    args = parser.parse_args()
    print(json.dumps(inspect(args.corpus), indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
