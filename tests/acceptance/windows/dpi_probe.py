#!/usr/bin/env python3
"""DPI-unaware owned target on the real scaled primary display, without changing OS settings."""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import traceback

from cursor_probe import Mcp, read, require, wait, write


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--dtw',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    args=p.parse_args();require(os.name=='nt' and sys.flags.utf8_mode,'Use Windows: python -X utf8')
    from PIL import Image, ImageChops
    binary=args.dtw.resolve(strict=True);out=args.output.resolve();require(not out.exists(),'Use new output');out.mkdir(parents=True)
    summary={'passed':False,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'cases':[],'cleanup':[]}
    client=None;fixture=None
    try:
        compile=subprocess.run(['powershell.exe','-NoProfile','-ExecutionPolicy','Bypass','-File',str(Path(__file__).with_name('compile-cursor.ps1')),
            '-OutputDirectory',str(out/'build')],capture_output=True)
        (out/'compile.stdout').write_bytes(compile.stdout);(out/'compile.stderr').write_bytes(compile.stderr);require(compile.returncode==0,'Compile failed')
        directory=out/'fixture';directory.mkdir()
        fixture=subprocess.Popen([str(out/'build/Fixture.exe'),str(directory),'A','unaware'])
        state=wait(lambda:read(directory/'state.json'));require(state['dpi']==96 and state['dpi_awareness_set'],'Target was not DPI unaware')
        client=Mcp(binary,out/'mcp');catalog=client.tool('apps',{})
        app=next(a['app'] for a in catalog['apps'] if a['app'].casefold()==str(out/'build/Fixture.exe').casefold())
        capture,_=client.execute(app,'capture-unaware','const w=(await app.windows()).windows[0];state.window=w.window_ref;await app.focus({window_ref:state.window});return await app.capture({window_ref:state.window});')
        def snapshot(name):
            path=out/(name+'.json');subprocess.run([str(out/'build/CursorObserver.exe'),'--snapshot',str(path),state['hwnd']],check=True,
                                                    creationflags=subprocess.CREATE_NO_WINDOW)
            return read(path)
        before=snapshot('physical-before');require(before['foreground_before'] and before['foreground_after'] and before['desktop_image'],'Owned target not foreground')
        native=Image.open(client.images[-1]).convert('RGB');physical=Image.open(before['desktop_image']).convert('RGB')
        require(native.size==physical.size,'Native capture dimensions are not physical window dimensions')
        def pad(im):
            mask=Image.new('L',im.size,255)
            for channel,value in zip(im.split(),(173,216,230)):
                mask=ImageChops.multiply(mask,channel.point([255 if i==value else 0 for i in range(256)]))
            return mask.getbbox()
        a,b=pad(native),pad(physical)
        require(a and b and all(abs(x-y)<=1 for x,y in zip(a,b)),'Native and physical pointer pad coordinates differ')
        x,y=(a[0]+a[2])//2,(a[1]+a[3])//2
        client.execute(app,'physical-coordinate-click',f'return await app.click({{window_ref:state.window,observation_ref:{json.dumps(capture["observation_ref"])},x:{x},y:{y}}});')
        effect=wait(lambda:(s if (s:=read(directory/'state.json')) and s['left_up']==state['left_up']+1 else None))
        def colored():
            s=snapshot('physical-after');rows=[w for w in s['windows'] if w['parent']==client.proc.pid and w['visible'] and max(w['palette_pixels'])>=20]
            return (s,rows[0]) if len(rows)==1 else None
        after,marker=wait(colored)
        require(marker['dpi']>96,'This control needs an actually scaled monitor; it cannot certify scale on a 100% display')
        require(marker['bounds']['width']==math.ceil(94*marker['dpi']/96) and marker['bounds']['height']==math.ceil(68*marker['dpi']/96),
                'Cursor raster followed the unaware target DPI instead of monitor DPI')
        require(not marker['enabled'] and after['foreground_after'],'Cursor changed activation/input behavior')
        summary.update(target_dpi=state['dpi'],monitor_helper_dpi=marker['dpi'],physical_size=list(physical.size),native_pad=a,
                       physical_pad=b,marker_size=marker['bounds'],click_effect=effect['left_up']-state['left_up'])
        summary['cases']=['unaware-target-native-capture-matches-physical-geometry','physical-pixel-click-exactly-once','cursor-raster-uses-monitor-scale']
        summary['passed']=True
    except BaseException:summary['failure']=traceback.format_exc()
    finally:
        if client:
            try:summary['cleanup'].append(client.close())
            except BaseException:summary['passed']=False;summary['cleanup'].append({'mcp_error':traceback.format_exc()})
        if fixture:
            (directory/'stop').write_text('stop',encoding='utf-8')
            try:
                code=fixture.wait(timeout=5);summary['cleanup'].append({'fixture_exit':code});summary['passed'] &= code==0
            except subprocess.TimeoutExpired:fixture.kill();summary['passed']=False;summary['cleanup'].append({'fixture_forced':True})
        write(out/'summary.json',summary);print(json.dumps(summary))
    return 0 if summary['passed'] else 1


if __name__=='__main__':raise SystemExit(main())
