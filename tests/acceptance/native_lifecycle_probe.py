#!/usr/bin/env python3
"""macOS helper owns its lease while waiting, and exits when its parent dies."""
import fcntl, os, subprocess, sys, time
if sys.platform != 'darwin': raise SystemExit(0)
code = "import subprocess,sys,time; p=subprocess.Popen([sys.argv[1],'--dtw-scene-helper'],stdin=subprocess.PIPE,stdout=subprocess.DEVNULL); print(p.pid,flush=True); time.sleep(30)"
owner = subprocess.Popen([sys.executable, '-c', code, sys.argv[1]], stdout=subprocess.PIPE, text=True)
helper = int(owner.stdout.readline())
# Match the runtime's private lock mode even if the probe wins creation.
lock = os.fdopen(os.open('/private/tmp/dev.caelis.dtw-scene-'+str(os.geteuid())+'.lock',
                         os.O_RDWR | os.O_CREAT, 0o600), 'a+')
try:
    # Wait until this helper acquires the scene lease before killing its parent.
    deadline = time.monotonic()+3
    locked = False
    while time.monotonic()<deadline:
        try: fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB); fcntl.flock(lock,fcntl.LOCK_UN)
        except BlockingIOError: locked = True; break
        time.sleep(.025)
    assert locked, 'helper never owned lease'
    owner.kill(); owner.wait(timeout=3)
    deadline = time.monotonic()+5
    while time.monotonic()<deadline:
        state=subprocess.run(['ps','-o','stat=','-p',str(helper)],capture_output=True,text=True).stdout.strip()
        if not state or state.startswith('Z'): break
        time.sleep(.025)
    else: raise AssertionError('orphan native helper remained live')
    fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
    print('{"helper_parent_death":"stopped","scene_lease":"released_after_exit"}')
finally:
    if owner.poll() is None: owner.kill(); owner.wait()
    lock.close()
