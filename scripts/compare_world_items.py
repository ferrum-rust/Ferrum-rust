"""Capture the actual 1.8.9 held/dropped item renderers and the same scenes through Vulkan."""
import argparse
import gzip
import json
import shutil
import subprocess
import tempfile
from pathlib import Path
import numpy as np
from PIL import Image
from compare_creative_inventory import isolated_profile
from test_vanilla_menus import capture
from vanilla_world import ROOT


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary",type=Path,required=True)
    parser.add_argument("--reference",type=Path,help="Existing WorldItemModelOracle screenshots")
    parser.add_argument("--parent",type=Path,default=Path(tempfile.gettempdir()))
    parser.add_argument("--report",type=Path,default=ROOT/"reports/world-items-189")
    parser.add_argument("--refresh",action="store_true",help="Regenerate original mesh and transform fixtures")
    a=parser.parse_args()
    ref=a.reference
    if ref:
        if a.refresh:parser.error("--refresh requires a fresh reference run")
    else:
        original,data=capture("WorldItemModelOracle",a.parent)
        ref=original/"game/screenshots"
        if a.refresh:
            (ROOT/"src/vanilla/world_item_models189.json.gz").write_bytes(gzip.compress(data.read_bytes(),9,mtime=0))
            shutil.copyfile(original/"transform-checks.json",ROOT/"tests/fixtures/vanilla189-world-item-transforms.json")
    work,jar,env=isolated_profile(a.parent,{"gui_scale":2,"language":"ko_kr","volume":0,"fov":70,
        "clouds":0,"entity_shadows":False,"render_distance":4})
    out=a.report.resolve();out.mkdir(parents=True,exist_ok=True)
    rows=[]
    for name in ["item-reference","held-grass","held-furnace","held-stairs","held-pickaxe","dropped-items"]:
        path=out/(name+".png")
        subprocess.run([str(a.binary.resolve()),"--jar",str(jar),"--fresh","--creative","--generator","1",
                        "--screen",name,"--screenshot",str(path)],cwd=work,env=env,check=True,timeout=90)
        actual=np.array(Image.open(path).convert("RGB"))
        if name=="item-reference":
            background=actual;continue
        vanilla=np.array(Image.open(ref/(name+".png")).convert("RGB"))
        original_mask=(vanilla!=[255,0,255]).any(2)
        # Restrict native difference-from-background to the object region (excluding HUD).
        # Keep the full raw Rust frame as well, for inspecting the normal renderer.
        ys,xs=np.where(original_mask)
        x0,x1=max(0,xs.min()-25),min(1600,xs.max()+26)
        y0,y1=max(0,ys.min()-25),min(900,ys.max()+26)
        crop=(slice(y0,y1),slice(x0,x1))
        predicted=(actual[crop]!=background[crop]).any(2)
        original_mask=original_mask[crop]
        vanilla=vanilla[crop]; actual=actual[crop]
        solid=np.full(vanilla.shape,49,dtype=np.uint8)
        left=np.where(original_mask[:,:,None],vanilla,solid)
        right=np.where(predicted[:,:,None],actual,solid)
        Image.fromarray(np.concatenate([left,right],axis=1)).save(out/("compare-"+name+".png"))
        intersection=predicted & original_mask
        rows.append(dict(screen=name,original_pixels=int(original_mask.sum()),rust_pixels=int(predicted.sum()),
                         silhouette_iou=round(float(intersection.sum()/(predicted|original_mask).sum()),6),
                         mean_rgb_delta_on_overlap=round(float(abs(actual.astype(float)-vanilla.astype(float))[intersection].mean()),3) if intersection.any() else None))
    (out/"comparison.json").write_text(json.dumps({"resolution":[1600,900],"fov":70,"columns":"Original 1.8.9 | Rust/Vulkan, neutral background for model comparisons. Independent native full frames included.","screens":rows},indent=2)+"\n",encoding="utf8")
    print(rows)

if __name__=="__main__":
    main()
