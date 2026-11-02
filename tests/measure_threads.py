#!/usr/bin/env python3
"""Private-bus collector/cold-tray CPU, memory and thread observation.

Build release binaries and the debug tray_fixture example first. Results remain
at the explicitly requested local path; do not publish machine diagnostics.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BIN = Path(os.environ.get('IGPU_MAN_BIN_DIR', ROOT/'target/release'))


def usage(pid):
    proc = Path('/proc')/str(pid)
    fields = (proc/'stat').read_text().rsplit(')', 1)[1].split()
    status = dict(line.split(':', 1) for line in (proc/'status').read_text().splitlines() if ':' in line)
    return {'ticks':sum(int(fields[i]) for i in [11,12]),
            'ticks_with_reaped_helpers':sum(int(fields[i]) for i in [11,12,13,14]),
            'threads':int(status['Threads']),
            'rss_mib':int(status['VmRSS'].split()[0])/1024,
            'virtual_mib':int(status['VmSize'].split()[0])/1024}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    parser.add_argument('--seconds', type=float, default=35)
    args = parser.parse_args()
    if not 10 <= args.seconds <= 120:
        parser.error('Use 10–120 seconds per observation')
    with tempfile.TemporaryDirectory(prefix='sidecar-thread-measure-') as temp:
        env = {k:v for k,v in os.environ.items() if not k.startswith('IGPU_MAN_SCREENSHOT_')}
        env.update(XDG_CONFIG_HOME=temp+'/config',XDG_DATA_HOME=temp+'/data',XDG_STATE_HOME=temp+'/state')
        children = []
        def run(name, *options):
            return subprocess.run([BIN/name,*options],env=env,capture_output=True,text=True,timeout=5)
        def wait_ready(name, *options):
            for _ in range(150):
                result=run(name,*options)
                if result.returncode == 0:
                    value=json.loads(result.stdout)
                    if name=='igpu-man' or value.get('tray_resident'):return value
                assert all(p.poll() is None for p in children),'Fixture process exited'
                time.sleep(.04)
            raise AssertionError('Private fixture did not become ready')
        # Never forward control to an already running collector/dashboard.
        if run('igpu-man','status','--json').returncode == 0:
            raise RuntimeError('Existing collector left untouched; use dbus-run-session')
        existing=json.loads(run('igpu-man-desktop','--status').stdout)
        if existing.get('running') is not False:
            raise RuntimeError('Existing dashboard left untouched; use dbus-run-session')
        try:
            children.append(subprocess.Popen([BIN/'igpu-mand'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL))
            wait_ready('igpu-man','status','--json')
            children.append(subprocess.Popen([ROOT/'target/debug/examples/tray_fixture'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL))
            children.append(subprocess.Popen([BIN/'igpu-man-desktop','--tray'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL))
            status=wait_ready('igpu-man-desktop','--status')
            assert status.get('pid')==children[2].pid,'Unexpected dashboard owner'
            assert all(p.poll() is None for p in children),'Fixture process exited'
            time.sleep(1)
            observed={'collector':children[0].pid,'cold_tray':children[2].pid}
            before={name:usage(pid) for name,pid in observed.items()}
            started=time.monotonic();time.sleep(args.seconds);elapsed=time.monotonic()-started
            assert all(p.poll() is None for p in children),'Fixture exited during observation'
            after={name:usage(pid) for name,pid in observed.items()}
            final=json.loads(run('igpu-man-desktop','--status').stdout)
            assert final.get('pid')==children[2].pid,'Dashboard owner changed'
            assert final['windows_created']==0 and final['updates']==0
            assert final['telemetry_wakeups']==status['telemetry_wakeups']
            hz=os.sysconf('SC_CLK_TCK');report={
                'seconds':elapsed,
                'limits':'One private-bus observation on an uncontrolled desktop; one CPU core = 100%. Helpers include only reaped children. Virtual memory is address-space reservation, not physical RAM. Not a game benchmark or a universal thread/CPU bound. Dependency runtime and on-demand blocking threads remain.',
                'binaries_sha256':{name:hashlib.file_digest((BIN/name).open('rb'),'sha256').hexdigest() for name in ['igpu-mand','igpu-man-desktop']},
                'processes':{},'cold_tray_windows':final['windows_created'],'cold_tray_updates':final['updates'],'cold_tray_telemetry_wakeups':final['telemetry_wakeups']}
            for name,values in after.items():
                report['processes'][name]={'before':before[name],'after':values,
                    'cpu_percent_one_core':(values['ticks']-before[name]['ticks'])/hz/elapsed*100,
                    'cpu_with_reaped_helpers_percent':(values['ticks_with_reaped_helpers']-before[name]['ticks_with_reaped_helpers'])/hz/elapsed*100}
            fd=os.open(args.output,os.O_WRONLY|os.O_CREAT|os.O_TRUNC|os.O_NOFOLLOW,0o600)
            with os.fdopen(fd,'w') as output:
                os.fchmod(output.fileno(),0o600)
                output.write(json.dumps(report,indent=2)+'\n')
            print(json.dumps(report),flush=True)
            result=run('igpu-man-desktop','--quit')
            assert result.returncode==0
            children[2].wait(timeout=5)
        finally:
            for child in reversed(children):
                if child.poll() is None:child.terminate();child.wait(timeout=5)


if __name__=='__main__':
    main()
