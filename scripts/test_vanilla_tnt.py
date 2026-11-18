"""Regenerate TNT motion, blast footprints, exposure and resistance from the installed 1.8.9 game."""
import gzip
import json
import os
from pathlib import Path
import subprocess
import tempfile

from vanilla_world import ROOT, classpath, java_executable

def main():
    cp = classpath(Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar")
    with tempfile.TemporaryDirectory(prefix="vanilla189-tnt-") as directory:
        work = Path(directory)
        sources = [ROOT / "tools/java" / (name+".java") for name in ("VanillaWorldBridge","RuntimeUseOracle","TntOracle")]
        subprocess.run(["javac","--release","8","-encoding","UTF-8","-cp",cp,"-d",directory,*sources],check=True)
        subprocess.run([java_executable(),"-Xmx512m","-cp",directory+os.pathsep+cp,"TntOracle",
                        str(work/"data.json"),directory],cwd=work,check=True)
        data = json.loads((work/"data.json").read_text(encoding="utf-8"))
    resistance = data.pop("resistance")
    values = ["    "+", ".join(repr(float(value)) for value in resistance[i:i+12])+"," for i in range(0,len(resistance),12)]
    (ROOT/"src/vanilla/blast_resistance.rs").write_text("// Original block registry, Block.getExplosionResistance(null).\nconst RESISTANCE: [f32; 256] = [\n"+"\n".join(values)+"\n];\n", encoding="utf-8")
    (ROOT/"tests/fixtures/vanilla189-tnt.json.gz").write_bytes(gzip.compress(json.dumps(data,separators=(",",":")).encode(),mtime=0))
    print(len(data["motion"]), "TNT ticks,", len(data["blasts"]), "blast scenes")

if __name__ == "__main__":
    main()
