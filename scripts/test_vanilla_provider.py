"""Record actual original provider/Chunk event order without running a server."""
import hashlib,json,os,subprocess
from pathlib import Path
from vanilla_world import ROOT,classpath,java_executable
jar=Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar';cp=classpath(jar)
classes=ROOT/'.local/oracles/provider/classes';classes.mkdir(parents=True,exist_ok=True)
subprocess.run(['javac','--release','8','-cp',cp,'-d',str(classes),str(ROOT/'tools/java/VanillaWorldBridge.java'),str(ROOT/'tools/java/ProviderOracle.java')],check=True)
result=subprocess.check_output([java_executable(),'-cp',str(classes)+os.pathsep+cp,'ProviderOracle',str(ROOT/'.local/oracles/provider/world')],cwd=ROOT)
data=json.loads(result);data['original_jar_sha256']=hashlib.sha256(jar.read_bytes()).hexdigest()
dest=ROOT/'tests/fixtures/vanilla189-provider.json';dest.write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
print(f'{len(data["cases"])} original provider traces / {sum(len(c["events"]) for c in data["cases"])} events -> {dest}')
