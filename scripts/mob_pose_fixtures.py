"""Export original creature poses and secondary meshes from the installed 1.8.9 client."""
import gzip
import json
import tempfile
from pathlib import Path

from test_vanilla_menus import capture
from vanilla_world import ROOT


def topology(frame):
    """Pose interpolation requires each vertex to retain its texture coordinate."""
    return [tuple(triangle[i] for i in (3, 4, 8, 9, 13, 14)) for triangle in frame]


def validate_frames(label, frames):
    reference = topology(frames[0])
    for index, frame in enumerate(frames[1:], 1):
        if topology(frame) != reference:
            raise RuntimeError(f"{label} frame {index} changed triangle/UV ordering")


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="ferrum-mob-poses-") as directory:
        _, base_output = capture("RemoteMobOracle", Path(directory))
        _, pose_output = capture("MobPoseOracle", Path(directory))
        base = json.loads(base_output.read_text(encoding="utf8"))
        poses = json.loads(pose_output.read_text(encoding="utf8"))
        for entity_id, species in base.items():
            validate_frames(f"base {entity_id}", species["frames"])
        for entity_id, frames in poses.items():
            validate_frames(f"pose {entity_id}", frames)
        if topology(base["63"]["frames"][0]) != topology(poses["63"][0]):
            raise RuntimeError("Dragon base and animated poses have different topology")
        for name, output in (("mob_models189", base_output), ("mob_poses189", pose_output)):
            (ROOT / f"src/vanilla/{name}.json.gz").write_bytes(
                gzip.compress(output.read_bytes(), mtime=0))
        print(f"Validated {len(base)} base models and {len(poses)} pose sets; "
              f"dragon: {len(base['63']['frames'][0])} triangles in every frame")
