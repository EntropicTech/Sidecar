#!/usr/bin/env python3
"""Real native window destruction/recreation on private SNI or actual Plasma."""
from fixture_bus import require_empty
import argparse,json,os,subprocess,tempfile,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
BIN=Path(os.environ.get('IGPU_MAN_BIN_DIR',ROOT/'target/debug'))
def main():
    parser=argparse.ArgumentParser();parser.add_argument('--live',action='store_true');parser.add_argument('--report',type=Path);args=parser.parse_args()
    if not args.live:require_empty()
    results={'live_plasma':args.live,'cycles':[]}
    with tempfile.TemporaryDirectory(prefix='sidecar-tray-') as temporary:
        env={k:v for k,v in os.environ.items() if not k.startswith('IGPU_MAN_SCREENSHOT_')}
        env.update(XDG_CONFIG_HOME=temporary+'/config',XDG_DATA_HOME=temporary+'/data',XDG_STATE_HOME=temporary+'/state')
        def gui(*options):return subprocess.check_output([str(BIN/'igpu-man-desktop'),*options],env=env,text=True,timeout=5)
        def status():return json.loads(gui('--status'))
        def wait(predicate):
            for _ in range(150):
                s=status()
                if s.get("running"):assert s.get("pid")==child.pid,"Unexpected dashboard owner"
                if predicate(s):return s
                assert child.poll() is None,Path(temporary+'/gui.stderr').read_text()[-5000:]
                time.sleep(.04)
            raise AssertionError(s)
        assert status().get('running') is False,'Existing dashboard left untouched'
        watcher=None;child=None
        try:
            if not args.live:
                watcher=subprocess.Popen([str(ROOT/'target/debug/examples/tray_fixture')],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
                time.sleep(.2)
            with open(temporary+'/gui.stderr','w') as log:
                child=subprocess.Popen([str(BIN/'igpu-man-desktop')],env=env,stdout=log,stderr=log)
            wait(lambda s:s.get('visible') and s.get('tray_available'))
            service=f'org.kde.StatusNotifierItem-{child.pid}-1'
            def activate():
                subprocess.run(['busctl','--user','call',service,'/StatusNotifierItem','org.kde.StatusNotifierItem','Activate','ii','0','0'],env=env,check=True,capture_output=True,timeout=3)
            gui('--applications');wait(lambda s:s.get('view')==1)
            for i in range(6):
                started=time.monotonic();gui('--close');wait(lambda s:s.get('tray_resident'))
                time.sleep(.15);a=status();time.sleep(.35);b=status()
                assert a['updates']==b['updates'],'Window kept rendering while closed'
                assert not b['quitting'] and b['pid']==child.pid
                activate();s=wait(lambda s:s.get('visible') and s.get('updates',0)>b['updates'])
                assert s['view']==1 and s['pid']==child.pid
                results['cycles'].append({'cycle':i+1,'close_activate_seconds':round(time.monotonic()-started,3),'same_pid':True,'parked_updates_stopped':True})
            # Reopen accepted while the previous window is still being torn down.
            for _ in range(16):
                gui('--close');gui();wait(lambda s:s.get('visible') and not s.get('quitting'))
            gui('--close');wait(lambda s:s.get('tray_resident'))
            if watcher:
                watcher.terminate();watcher.wait(timeout=3);watcher=None
                wait(lambda s:s.get('visible') and not s.get('tray_available'))
                watcher=subprocess.Popen([str(ROOT/'target/debug/examples/tray_fixture')],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
                wait(lambda s:s.get('tray_available'))
                results['watcher_loss_reopens']=True
                results['watcher_return_recovers']=True
                gui('--close');wait(lambda s:s.get('tray_resident'))
            raw=subprocess.check_output(['busctl','--user','--json=short','call',service,'/MenuBar','com.canonical.dbusmenu','GetLayout','iias','0','1','0'],env=env,text=True,timeout=3)
            entries=json.loads(raw)['data'][1][2]
            item=next(e['data'][0] for e in entries if e['data'][1].get('label',{}).get('data')=='Quit Sidecar')
            subprocess.run(['busctl','--user','call',service,'/MenuBar','com.canonical.dbusmenu','Event','isvu',str(item),'clicked','i','0','0'],env=env,check=True,capture_output=True,timeout=3)
            assert child.wait(timeout=5)==0 and status().get('running') is False
            results['quit_from_tray_while_parked']=True
            print(json.dumps(results,indent=2));print('PASS: close parks, Activate reopens with same state/PID, no parked redraw, rapid Show survives, tray Quit fully exits')
        finally:
            if child and child.poll() is None:child.terminate();child.wait(timeout=5)
            if watcher and watcher.poll() is None:watcher.terminate();watcher.wait(timeout=5)
    if args.report:args.report.write_text(json.dumps(results,indent=2)+'\n')
if __name__=='__main__':main()
