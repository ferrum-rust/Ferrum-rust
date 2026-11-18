"""Import original Korean 1.8.9 menu layout values. Rendered PNGs stay in a disposable test game."""
import argparse
import json
import os
import subprocess
import tempfile
from pathlib import Path

from vanilla_world import ROOT, classpath, java_executable


def capture(oracle, parent, jvm_options=()):
    version = Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9"
    work = Path(tempfile.mkdtemp(prefix="original-" + oracle.lower() + "-", dir=parent))
    game, classes = work / "game", work / "classes"
    game.mkdir()
    classes.mkdir()
    cp = classpath(version / "1.8.9.jar")
    javac = os.environ.get("MC189_JAVAC", r"C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe")
    helpers = ("RemoteMobOracle",) if oracle in ("UtilityWindowOracle", "MobPoseOracle") else ()
    subprocess.run([javac, "-encoding", "UTF-8", "-cp", cp, "-d", str(classes),
                    *[str(ROOT / "tools/java" / (source + ".java")) for source in dict.fromkeys(("VanillaFrameBenchmark", "MenuOracle", *helpers, oracle))]], cwd=work, check=True)
    natives = version / "natives-windows-x86_64"
    output = work / "layouts.json"
    print(oracle, work, flush=True)
    with (work / "oracle.log").open("w", encoding="utf8") as log:
        subprocess.run([java_executable(), "-Xmx512M", f"-Djava.library.path={natives}",
                        f"-Dorg.lwjgl.librarypath={natives}", f"-Duser.home={work}", f"-Djava.io.tmpdir={work}",
                        *jvm_options, "-cp", str(classes) + os.pathsep + cp, oracle, str(game), str(version.parents[1] / "assets"),
                        str(output)], cwd=work, stdout=log, stderr=subprocess.STDOUT, timeout=120, check=True)
    return work, output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--parent", type=Path, default=Path(tempfile.gettempdir()))
    parser.add_argument("--pause", action="store_true", help="Capture pause, LAN and achievements against the singleplayer server")
    args = parser.parse_args()
    work, output = capture("PauseMenusOracle" if args.pause else "MenuOracle", args.parent)
    menus = json.loads(output.read_text(encoding="utf8"))
    if len(menus) != (8 if args.pause else 16) or any(item["size"] != [800, 450] for item in menus.values()):
        raise RuntimeError("Original menu count or GUI scale differs")
    if args.pause:
        registry = json.loads((work / "achievements.json").read_text(encoding="utf8"))
        if len(registry) != 34:
            raise RuntimeError("Original achievement registry length differs")
        (ROOT / "src/ui/achievements189.json").write_text(
            json.dumps(registry, ensure_ascii=False, separators=(",", ":")) + "\n", encoding="utf8")
    target = "vanilla189-pause-menus.json" if args.pause else "vanilla189-menu-layouts.json"
    (ROOT / "tests/fixtures" / target).write_text(
        json.dumps(menus, ensure_ascii=False, separators=(",", ":")) + "\n", encoding="utf8")
    print("Screenshots:", work / "game/screenshots")


if __name__ == "__main__":
    main()
