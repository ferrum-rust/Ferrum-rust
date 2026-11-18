"""Export original EntityItem constructor, merge, pickup and registry facts."""
import gzip
import json
import os
import subprocess
from pathlib import Path

from vanilla_world import ROOT, classpath, java_executable

cp=classpath(Path(os.environ["APPDATA"])/".minecraft/versions/1.8.9/1.8.9.jar")
classes=ROOT/".local/oracles/item-entities"
classes.mkdir(parents=True,exist_ok=True)
subprocess.run(["javac","--release","8","-cp",cp,"-d",str(classes),str(ROOT/"tools/java/FallingEntityOracle.java"),str(ROOT/"tools/java/ItemEntityOracle.java")],check=True)
fixture=ROOT/"tests/fixtures/vanilla189-item-entities.json.gz"
subprocess.run([java_executable(),"-Xmx512m","-cp",str(classes)+os.pathsep+cp,"ItemEntityOracle",str(fixture)],cwd=ROOT,check=True)
data=json.load(gzip.open(fixture,"rt",encoding="utf-8"))
facts=sorted(data["facts"])
(ROOT/"src/vanilla/item_entity_data.rs").write_text("/// Original registered item ID, maximum stack size, subtype flag and durability.\nconst ITEM_FACTS: &[[u16; 4]] = &[\n"+"".join(f"    {row},\n" for row in facts)+"];\n",encoding="utf-8")
