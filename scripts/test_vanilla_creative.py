"""Regenerate creative catalog facts and validate offline 1.8.9 reference screens."""
import argparse
import gzip
import json
import tempfile
from pathlib import Path
from test_vanilla_menus import capture
from vanilla_world import ROOT


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--parent", type=Path, default=Path(tempfile.gettempdir()))
    args = parser.parse_args()
    work, path = capture("CreativeInventoryOracle", args.parent)
    save(work,path)


def save(work, path):
    data = json.loads(path.read_text(encoding="utf8"))
    meshes, unique = [], {}
    for row in data["entries"]:
        key = json.dumps(row["mesh"], separators=(",", ":"))
        if key not in unique:
            unique[key] = len(meshes)
            meshes.append(row["mesh"])
        row["mesh"] = unique[key]
    data["meshes"] = meshes
    assert len(data["tabs"]) == 12 and len(data["tabs"][5][2]) == 678
    target = ROOT / "src/ui/creative_catalog.json.gz"
    target.write_bytes(gzip.compress(json.dumps(data, ensure_ascii=False, separators=(",", ":")).encode(), mtime=0))
    print(f"{len(data['entries'])} catalog entries and {len(meshes)} meshes. Original screenshots in {work/'game/screenshots'}")


if __name__ == "__main__":
    main()
