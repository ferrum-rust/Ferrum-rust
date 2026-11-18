"""Capture original VisGraph face connections and camera exits without a GPU."""
import os
from pathlib import Path
import subprocess
from vanilla_world import ROOT, classpath, java_executable
cp = classpath(Path(os.environ["APPDATA"]) / ".minecraft/versions/1.8.9/1.8.9.jar")
out = ROOT/".local/oracles/section-visibility"
out.mkdir(parents=True,exist_ok=True)
subprocess.run(["javac","--release","8","-cp",cp,"-d",str(out),str(ROOT/"tools/java/SectionVisibilityOracle.java")],check=True)
subprocess.run([java_executable(),"-cp",str(out)+os.pathsep+cp,"SectionVisibilityOracle",
                str(ROOT/"tests/fixtures/vanilla189-section-visibility.json.gz")],check=True)
