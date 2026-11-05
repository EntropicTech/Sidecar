#!/usr/bin/env python3
"""Actual KWin window retirement, independent of Sidecar's status flags.

python3 tests/wayland_window_lifecycle.py --live --cycles 6
Needs Plasma Wayland/qdbus6. Refuses an existing real dashboard; runs fixture
names/state on a private bus. Only owned fixture windows are queried/closed by
short-lived KWin scripts; scripts are unloaded and no settings are changed.
"""
import argparse
import json
import os
from pathlib import Path
import secrets
import signal
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BIN = Path(os.environ.get('IGPU_MAN_BIN_DIR', ROOT/'target/debug'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--live',action='store_true',required=True)
    parser.add_argument('--cycles',type=int,default=6,choices=range(1,21))
    parser.add_argument('--idle-seconds',type=int,default=0,choices=range(0,61))
    args = parser.parse_args()
    if not os.environ.get('WAYLAND_DISPLAY'):raise RuntimeError('A live Plasma Wayland session is required')
    def run(command, **kwargs):
        return subprocess.run(list(map(str,command)),capture_output=True,text=True,check=True,timeout=8,**kwargs)
    env = {k:v for k,v in os.environ.items() if not k.startswith('IGPU_MAN_SCREENSHOT_')}
    if json.loads(run([BIN/'igpu-man-desktop','--status'],env=env).stdout).get('running') is not False:
        raise RuntimeError('Existing real dashboard left untouched; close it before this explicit live test')
    with tempfile.TemporaryDirectory(prefix='sidecar-kwin-lifecycle-') as temporary:
        root = Path(temporary)
        bus = run(['dbus-daemon','--session','--print-address=1','--print-pid=1','--fork']).stdout.splitlines()
        buspid = int(bus[1])
        fixture = dict(env,DBUS_SESSION_BUS_ADDRESS=bus[0],XDG_CONFIG_HOME=str(root/'config'),
            XDG_DATA_HOME=str(root/'data'),XDG_STATE_HOME=str(root/'state'))
        children = []
        def gui(*options):return run([BIN/'igpu-man-desktop',*options],env=fixture)
        def status():return json.loads(gui('--status').stdout)
        def wait(predicate):
            for _ in range(120):
                state = status()
                if predicate(state):return state
                assert child.poll() is None,'Owned dashboard exited'
                time.sleep(.025)
            raise AssertionError('Owned dashboard state transition timed out')
        def windows(close=False):
            # Verify ownership before a script may act; do not close by title or focus.
            assert child.poll() is None
            token = 'sidecar_retire_'+secrets.token_hex(8)
            path = root/(token+'.js')
            path.write_text('var wins = workspace.windowList().filter(w => w.pid === '+str(child.pid)+');\n'
                'print('+json.dumps(token+' ')+
                '+ JSON.stringify(wins.map(w => ({deleted:w.deleted,hidden:w.hidden,unresponsive:w.unresponsive}))));\n'
                + ('wins.forEach(w => w.closeWindow());\n' if close else ''))
            sid = run(['qdbus6','org.kde.KWin','/Scripting','org.kde.kwin.Scripting.loadScript',path,token],env=env).stdout.strip()
            assert sid.isdigit(),'KWin did not load the owned fixture script'
            try:
                paths = run(['qdbus6','org.kde.KWin'],env=env).stdout.splitlines()
                script = next(p.strip() for p in paths if p.strip().endswith('/Script'+sid) or p.strip()=='/'+sid)
                run(['qdbus6','org.kde.KWin',script,'org.kde.kwin.Script.run'],env=env)
                for _ in range(30):
                    lines = run(['journalctl','--user','_COMM=kwin_wayland','--since','2 minutes ago',
                        '--grep',token,'-o','json','--no-pager'],env=env).stdout.splitlines()
                    for line in lines:
                        message = json.loads(line).get('MESSAGE') or ''
                        if token+' ' in message:
                            return [w for w in json.loads(message.split(token+' ',1)[1]) if not w['deleted']]
                    time.sleep(.025)
                raise AssertionError('KWin fixture probe response missing')
            finally:
                assert run(['qdbus6','org.kde.KWin','/Scripting','org.kde.kwin.Scripting.unloadScript',token],env=env).stdout.strip()=='true'
        def expect_windows(count):
            for _ in range(40):
                result = windows()
                if len(result)==count:return
                time.sleep(.025)
            raise AssertionError(f'Compositor still has {len(result)} fixture windows; expected {count}')
        try:
            children.append(subprocess.Popen([ROOT/'target/debug/examples/tray_fixture'],env=fixture,
                stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL))
            child = subprocess.Popen([BIN/'igpu-man-desktop','--tray'],env=fixture,
                stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            children.append(child)
            wait(lambda s:s.get('tray_resident'))
            expect_windows(0)
            for cycle in range(args.cycles):
                gui('--applications')
                wait(lambda s:s.get('visible'))
                expect_windows(1)
                if cycle%2 == 0:windows(close=True)
                else:gui('--close')
                parked = wait(lambda s:s.get('tray_resident'))
                assert not parked.get('native_window_active',False)
                expect_windows(0)
                time.sleep(.25)
                after = status()
                assert after['pid']==child.pid and after['updates']==parked['updates']
                expect_windows(0)
            if args.idle_seconds:
                def usage():
                    fields = Path(f'/proc/{child.pid}/stat').read_text().rsplit(')',1)[1].split()
                    return int(fields[11])+int(fields[12])
                before = status();ticks = usage();started = time.monotonic()
                time.sleep(args.idle_seconds)
                elapsed = time.monotonic()-started
                assert child.poll() is None,'Parked fixture exited during the idle observation'
                after = status()
                assert after['updates']==before['updates'] and after['telemetry_wakeups']==before['telemetry_wakeups']
                expect_windows(0)
                print(json.dumps({'parked_seconds':elapsed,'cpu_percent_one_core':(usage()-ticks)/os.sysconf('SC_CLK_TCK')/elapsed*100,
                    'parked_updates_stopped':True,'telemetry_wakeups_stopped':True,'compositor_windows':0,
                    'limits':'Single local observation; no universal CPU/RSS or gaming claim'}),flush=True)
            gui('--quit')
            assert child.wait(timeout=5)==0
            print(f'PASS: {args.cycles} native/API close cycles; KWin confirms zero retired windows, same-process reopening, no parked redraw, clean Quit')
        finally:
            for process in reversed(children):
                if process.poll() is None:process.terminate();process.wait(timeout=5)
            try:os.kill(buspid,signal.SIGTERM)
            except ProcessLookupError:pass


if __name__ == '__main__':main()
