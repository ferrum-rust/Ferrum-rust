"""Regenerate an oracle corpus from the owned original 1.8.9 noise bytecode.

The resulting JSON contains numerical outputs only. Rust tests compare every
floating-point bit pattern; no tolerance or lookalike threshold is used.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
from vanilla_world import ROOT, classpath


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--jar',type=Path,default=Path(os.environ.get('APPDATA',str(Path.home())))/'.minecraft/versions/1.8.9/1.8.9.jar')
    parser.add_argument('--output',type=Path,default=ROOT/'tests/fixtures/vanilla189-noise.json')
    args=parser.parse_args()
    cp=classpath(args.jar)
    classes=ROOT/'.local/vanilla-noise-oracle/classes';classes.mkdir(parents=True,exist_ok=True)
    source=ROOT/'tools/java/NoiseOracle.java'
    subprocess.run(['javac','-encoding','UTF-8','--release','8','-cp',cp,'-d',str(classes),str(source)],check=True)
    result=subprocess.run(['java','-cp',str(classes)+os.pathsep+cp,'NoiseOracle'],check=True,capture_output=True,text=True,encoding='utf-8')
    data=json.loads(result.stdout)
    data['original_jar_sha256']=hashlib.sha256(args.jar.read_bytes()).hexdigest()
    data['mapped_jar_sha256']=hashlib.sha256((ROOT/'.local/reference/1.8.9-named.jar').read_bytes()).hexdigest()
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(data,separators=(',',':'))+'\n',encoding='utf-8')
    doubles=sum(len(row.get('bits',[])) for row in data['cases'])
    print(f'Wrote {len(data["cases"])} Java oracle cases, {doubles} exact f64 values to {args.output}')


if __name__=='__main__':main()
