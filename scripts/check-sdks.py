#!/usr/bin/env python3
"""Cross-platform SDK check over the production supervisor and fixture-only UI."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
root = Path(__file__).resolve().parent.parent
def run(args, **kwargs):
    subprocess.run(args, cwd=root, check=True, **kwargs)
with tempfile.TemporaryDirectory(prefix='dtw-sdk-') as directory:
    helper = str(Path(directory) / ('dtw-fixture.exe' if os.name=='nt' else 'dtw-fixture'))
    env = {**os.environ, 'GOWORK':'off', 'DTW_SDK_TEST_HELPER':helper, 'PYTHONPATH':str(root/'clients/python')}
    run(['go','test','-c','-o',helper,'./cmd/dtw'], env=env)
    npm=shutil.which('npm.cmd' if os.name=='nt' else 'npm')
    run([npm,'ci','--prefix','clients/typescript'], env=env)
    run([npm,'test','--prefix','clients/typescript'], env=env)
    run([sys.executable,'-m','unittest','discover','-s','clients/python/tests','-v'], env=env)
    run(['cargo','test','--locked','--manifest-path','clients/rust/Cargo.toml'], env=env)
