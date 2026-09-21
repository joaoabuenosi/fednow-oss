#!/usr/bin/env python3
"""Merge the per-crate CycloneDX SBOMs cargo-cyclonedx emits into one document.

`cargo cyclonedx` writes one SBOM per workspace member (`core/fednow-core.cdx.json`
and friends). A release ships a single artifact, so this collapses them: every
workspace crate becomes a component of one aggregate BOM, the dependency graphs are
unioned, and duplicate dependencies shared between crates appear once.

Usage:
    merge-cyclonedx.py --output OUT --name NAME --version VERSION INPUT [INPUT ...]
"""

from __future__ import annotations

import argparse
import datetime
import json
import sys
import uuid


def component_key(component: dict) -> str:
    """Identity for de-duplication: bom-ref if present, else purl, else name@version."""
    return (
        component.get("bom-ref")
        or component.get("purl")
        or f"{component.get('name')}@{component.get('version')}"
    )


def merge(docs: list[dict], name: str, version: str) -> dict:
    components: dict[str, dict] = {}
    dependencies: dict[str, set[str]] = {}
    tools = None

    for doc in docs:
        metadata = doc.get("metadata", {})
        if tools is None:
            tools = metadata.get("tools")

        # The crate the SBOM describes is itself a component of the aggregate.
        root = metadata.get("component")
        if root is not None:
            # Its nested Cargo targets are an artefact of the per-crate layout;
            # the aggregate lists crates, not their rlib/bin targets.
            root = {k: v for k, v in root.items() if k != "components"}
            components.setdefault(component_key(root), root)

        for component in doc.get("components", []):
            components.setdefault(component_key(component), component)

        for entry in doc.get("dependencies", []):
            ref = entry.get("ref")
            if ref is None:
                continue
            dependencies.setdefault(ref, set()).update(entry.get("dependsOn", []))

    merged = {
        "bomFormat": "CycloneDX",
        "specVersion": docs[0].get("specVersion", "1.5"),
        "version": 1,
        "serialNumber": f"urn:uuid:{uuid.uuid4()}",
        "metadata": {
            "timestamp": datetime.datetime.now(datetime.timezone.utc)
            .replace(microsecond=0)
            .isoformat()
            .replace("+00:00", "Z"),
            "component": {
                "type": "application",
                "bom-ref": f"fednow-oss@{version}",
                "name": name,
                "version": version,
                "description": "FedNow OSS workspace: aggregate SBOM for the release",
                "licenses": [{"expression": "Apache-2.0"}],
                "externalReferences": [
                    {
                        "type": "vcs",
                        "url": "https://github.com/joaoabuenosi/fednow-oss",
                    }
                ],
            },
        },
        "components": sorted(
            components.values(),
            key=lambda c: (c.get("name") or "", c.get("version") or ""),
        ),
        "dependencies": [
            {"ref": ref, "dependsOn": sorted(depends_on)}
            for ref, depends_on in sorted(dependencies.items())
        ],
    }

    if tools is not None:
        merged["metadata"]["tools"] = tools

    return merged


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True)
    parser.add_argument("--name", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("inputs", nargs="+")
    args = parser.parse_args()

    docs = []
    for path in args.inputs:
        with open(path) as handle:
            docs.append(json.load(handle))

    merged = merge(docs, args.name, args.version)

    with open(args.output, "w") as handle:
        json.dump(merged, handle, indent=2)
        handle.write("\n")

    print(
        f"{args.output}: {len(merged['components'])} components "
        f"from {len(docs)} crate SBOMs"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
