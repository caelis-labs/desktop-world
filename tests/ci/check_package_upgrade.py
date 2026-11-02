#!/usr/bin/env python3
"""Rehearse a manager/runtime version update with two real compiled binaries.

The next patch is a test-only source copy, never a release candidate or tag.
No host configuration or GUI is touched.
"""
import argparse, json, os, re, shutil, subprocess, sys
from pathlib import Path

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--dtw',type=Path,required=True);parser.add_argument('--output',type=Path,required=True);args=parser.parse_args()
    repo=Path(__file__).resolve().parents[2];out=args.output.resolve();out.mkdir(parents=True,exist_ok=False)
    source=out/'next-source';shutil.copytree(repo/'cmd/dtw',source,ignore=shutil.ignore_patterns('target','__pycache__'))
    version=subprocess.check_output([str(args.dtw.resolve()),'--version'],encoding='utf-8').strip().split()[-1];major,minor,patch=map(int,version.split('.'));next_version=f'{major}.{minor}.{patch+1}'
    manifest=source/'Cargo.toml';text=manifest.read_text(encoding='utf-8');assert f'version = "{version}"' in text
    text=text.replace(f'version = "{version}"',f'version = "{next_version}"',1).replace('name = "dtw"','name = "dtw-next"',1);manifest.write_text(text,encoding='utf-8')
    lock=source/'Cargo.lock';text=lock.read_text(encoding='utf-8');text,count=re.subn(r'(name = "desktop-world"\nversion = ")[^"]+',r'\g<1>'+next_version,text,count=1);assert count==1;lock.write_text(text,encoding='utf-8')
    target=Path(os.environ['CARGO_TARGET_DIR']).resolve();subprocess.run(['cargo','+1.98.0','build','--release','--locked','--manifest-path',str(manifest),'--bin','dtw-next'],cwd=repo,check=True)
    future=target/'release'/('dtw-next.exe' if os.name=='nt' else 'dtw-next')
    assert subprocess.check_output([str(future),'--version'],encoding='utf-8').strip()==f'dtw {next_version}'
    subprocess.run([sys.executable,str(repo/'tests/acceptance/package_store_probe.py'),'--dtw',str(args.dtw.resolve()),'--next-dtw',str(future),'--output',str(out/'acceptance')],check=True)
    print(json.dumps({'passed':True,'baseline_version':version,'rehearsal_version':next_version,'meaning':'two compiled versions sharing store/lease protocol; next patch is test-only, not a published build'}))
if __name__=='__main__':main()
