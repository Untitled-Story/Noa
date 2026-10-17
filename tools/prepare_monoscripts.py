"""Prepare sidecar metadata from exported MonoScript records and explicit bindings.

This adapter understands serialized-objects.json; Noa itself only reads the
tool-independent schema. Bindings map exported GUID/fileID to sourceFile/PathID.
No script names, C# bodies or directory names are used to infer identities.
"""

import argparse
import json
from pathlib import Path
import re


def file_id(value):
    if not re.fullmatch(r"-?[1-9][0-9]*", value):
        raise ValueError(f"invalid canonical fileID: {value}")
    number = int(value)
    if not -(2**63) <= number < 2**63:
        raise ValueError(f"fileID outside int64: {value}")
    return value


def prepare(objects, bindings):
    index = {}
    for obj in objects["objects"]:
        key = (obj["sourceFile"], str(obj["pathID"]))
        if key in index:
            raise ValueError(f"duplicate source object: {key}")
        index[key] = obj
    scripts = {}
    for binding in bindings:
        if not isinstance(binding, (list, tuple)) or len(binding) != 4 or not all(isinstance(v, str) for v in binding):
            raise ValueError("binding must contain four strings: GUID, fileID, sourceFile, PathID")
        guid, exported_id, source_file, source_id = binding
        if not source_file.strip():
            raise ValueError("sourceFile must not be empty")
        if not re.fullmatch(r"[0-9a-fA-F]{32}", guid):
            raise ValueError(f"invalid GUID: {guid}")
        guid = guid.lower()
        if guid in {"0" * 32, *("0" * 16 + c + "0" * 15 for c in "def")}:
            raise ValueError(f"reserved GUID: {guid}")
        exported_id, source_id = file_id(exported_id), file_id(source_id)
        obj = index[(source_file, source_id)]
        if obj["classID"] != 115 or obj["type"] != "MonoScript":
            raise ValueError(f"source is not MonoScript: {source_file}/{source_id}")
        data = obj["data"]
        values = [data[k] for k in ("m_AssemblyName", "m_Namespace", "m_ClassName")]
        if not all(isinstance(v, str) for v in values) or not values[0].strip() or not values[2].strip():
            raise ValueError(f"incomplete MonoScript identity: {source_file}/{source_id}")
        record = {
            "guid": guid,
            "file_id": exported_id,
            "assembly": values[0],
            "namespace": values[1],
            "class_name": values[2],
            "source": {"asset": source_file, "file_id": source_id},
        }
        key = (guid, exported_id)
        if key in scripts and scripts[key] != record:
            raise ValueError(f"conflicting exported script reference: {key}")
        scripts[key] = record
    return {
        "format": "unity-monoscript-metadata",
        "version": 1,
        "scripts": [scripts[k] for k in sorted(scripts)],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--objects", required=True, type=Path)
    parser.add_argument("--bind", action="append", nargs=4,
                        metavar=("GUID", "EXPORTED_FILE_ID", "SOURCE_FILE", "SOURCE_PATH_ID"))
    parser.add_argument("--bindings", type=Path, help="JSON array of explicit GUID/fileID/sourceFile/PathID bindings")
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    bindings = list(args.bind or [])
    if args.bindings:
        bindings.extend(json.loads(args.bindings.read_text()))
    if not bindings:
        parser.error("at least one explicit binding is required")
    try:
        result = prepare(json.loads(args.objects.read_text()), bindings)
    except (ValueError, KeyError, TypeError) as error:
        parser.exit(1, f"INVALID_SCRIPT_METADATA: {error}\n")
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")


if __name__ == "__main__":
    main()
