#!/usr/bin/env python3
"""Screenshot-feature failure injection on private data; close/reopen must recover."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
ROOT=Path(__file__).resolve().parents[1]
BIN=ROOT/'target/debug'
def main():
    output=Path(sys.argv[1]).resolve();output.mkdir(parents=True,exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='igpu-man-workers-') as temporary:
        root=Path(temporary);env=dict(os.environ,XDG_CONFIG_HOME=str(root/'config'),XDG_DATA_HOME=str(root/'data'),XDG_STATE_HOME=str(root/'state'))
        daemon=subprocess.Popen([str(BIN/'igpu-mand')],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        desktop=None
        try:
            for _ in range(100):
                p=subprocess.run([str(BIN/'igpu-man'),'status','--json'],env=env,capture_output=True,timeout=5)
                if p.returncode==0:break
                time.sleep(.05)
            for case in ['recordings','telemetry','poison']:
                capture=dict(env,IGPU_MAN_SCREENSHOT_WORKER_FAILURE=case,IGPU_MAN_SCREENSHOT_TO=str(output/f'{case}.png'),IGPU_MAN_SCREENSHOT_WAIT_MS='1000')
                desktop=subprocess.Popen([str(BIN/'igpu-man-desktop')],env=capture,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
                failed=False
                for _ in range(100):
                    status=json.loads(subprocess.check_output([str(BIN/'igpu-man-desktop'),'--status'],env=env,text=True,timeout=3))
                    health=status.get('worker_health')
                    if health and ('failed' in health[:2] or health[2] is False):failed=True;break
                    if desktop.poll() is not None:
                        recent=[]
                        for log in (root/'state/igpu-man/logs').glob('desktop-*.jsonl'):
                            recent.extend(json.loads(line) for line in log.read_text().splitlines())
                        raise AssertionError((case,desktop.returncode,recent[-10:]))
                    time.sleep(.02)
                assert failed,(case,status)
                assert desktop.wait(timeout=10)==0 and (output/f'{case}.png').exists()
                assert json.loads(subprocess.check_output([str(BIN/'igpu-man-desktop'),'--status'],env=env,text=True))['running'] is False
            # Fresh view-model/worker state; collector was never restarted.
            capture=dict(env,IGPU_MAN_SCREENSHOT_TO=str(output/'reopened.png'),IGPU_MAN_SCREENSHOT_WAIT_MS='1000')
            desktop=subprocess.Popen([str(BIN/'igpu-man-desktop')],env=capture,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            for _ in range(100):
                status=json.loads(subprocess.check_output([str(BIN/'igpu-man-desktop'),'--status'],env=env,text=True,timeout=3))
                if status.get('ready') and status.get('worker_health')==['running','running',True]:break
                time.sleep(.02)
            else:raise AssertionError(status)
            assert desktop.wait(timeout=10)==0
            events=[]
            for path in (root/'state/igpu-man/logs').glob('desktop-*.jsonl*'):
                events.extend(json.loads(line) for line in path.read_text().splitlines())
            assert any(e['event']=='dashboard.worker_failed' for e in events)
            assert any(e['event']=='runtime.panic' for e in events)
            assert 'synthetic poisoned backend' not in json.dumps(events)
            assert daemon.poll() is None
            print('PASS: both worker failures/poison are visible, fallback view renders, Close exits cleanly, reopen recovers without restarting collector')
        finally:
            if desktop and desktop.poll() is None:desktop.terminate();desktop.wait(timeout=5)
            daemon.terminate();daemon.wait(timeout=10)
if __name__=='__main__':main()
