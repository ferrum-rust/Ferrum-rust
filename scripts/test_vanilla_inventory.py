"""Generate real Java 1.8.9 inventory traces, then compare native Rust slot operations.

The owned mapped JAR/dependencies are used only for this verification command.
No Java process is needed by the native game.
"""
import argparse
import os
from pathlib import Path
import subprocess
from vanilla_world import ROOT, classpath

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--jar',type=Path,default=Path(os.environ['APPDATA'])/'.minecraft/versions/1.8.9/1.8.9.jar')
    parser.add_argument('--fixtures-only',action='store_true')
    args=parser.parse_args()
    cp=classpath(args.jar)
    classes=ROOT/'.local/inventory-oracle/classes';classes.mkdir(parents=True,exist_ok=True)
    fixture=ROOT/'.local/reference/fixtures/inventory.json'
    subprocess.run(['javac','-encoding','UTF-8','-source','8','-target','8','-cp',cp,'-d',str(classes),str(ROOT/'tools/java/InventoryOracle.java')],check=True,cwd=ROOT)
    subprocess.run(['java','-Djava.awt.headless=true','-cp',str(classes)+os.pathsep+cp,'InventoryOracle',str(fixture)],check=True,cwd=ROOT)
    if not args.fixtures_only:
        cargo=Path.home()/'.cargo/bin/cargo.exe'
        subprocess.run([str(cargo),'test','--locked','vanilla_inventory_matches_original_bytecode','--','--include-ignored','--nocapture'],check=True,cwd=ROOT)

if __name__=='__main__':main()
