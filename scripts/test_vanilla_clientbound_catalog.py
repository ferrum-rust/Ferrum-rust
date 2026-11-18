"""Export one reader/writer-verified packet for each installed protocol-47 clientbound Play registration."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

from vanilla_world import ROOT, classpath, java_executable


def main():
    fixtures = ROOT / "tests/fixtures"
    def load(name):
        return json.loads((fixtures / f"vanilla189-{name}.json").read_text(encoding="utf-8"))
    seeds = {}
    for name, packet_id, hexdata in load("play-events"):
        if name != "update-sign":
            seeds.setdefault(packet_id, hexdata)
    for row in load("presentation-packets"):
        seeds.setdefault(row["id"], row["payload"])
    seeds[2] = load("chat-packets")["incoming0"]
    entities = load("entity-pick-packets")
    seeds[56] = entities["players"][0]["wire"]
    seeds[27] = entities["attach"][0]["wire"]
    effects = load("player-effect-packets")["packets"][0]
    seeds[29], seeds[30] = effects["add"], effects["remove"]
    cp = classpath(Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar")
    javac = os.environ.get("MC189_JAVAC", r"C:\Program Files\Java\jdk1.8.0_202\bin\javac.exe")
    with tempfile.TemporaryDirectory(prefix="catalog-189-") as work:
        seed_file = Path(work) / "seeds.json"
        seed_file.write_text(json.dumps(seeds),encoding="utf-8")
        subprocess.run([javac, "-encoding", "UTF-8", "-cp", cp, "-d", work,
                        ROOT / "tools/java/PlayEventsOracle.java", ROOT / "tools/java/ClientboundCatalogOracle.java"],
                        cwd=work, check=True)
        subprocess.run([java_executable(), "-cp", work + os.pathsep + cp, "ClientboundCatalogOracle",
                        seed_file, fixtures / "vanilla189-clientbound-catalog.json"], cwd=work, check=True)


if __name__ == "__main__":
    main()
