#!/usr/bin/env python3
"""Primary choose-app → use-iGPU → observe → Undo workflow, private bus/XDG only."""
from fixture_bus import require_empty, require_collector
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

ROOT=Path(__file__).resolve().parents[1]
BIN=Path(os.environ.get('IGPU_MAN_BIN_DIR',ROOT/'target/debug')).resolve()

def main():
    require_empty()
    with tempfile.TemporaryDirectory(prefix='sidecar-apps-') as temporary:
        root=Path(temporary)
        env=dict(os.environ,XDG_CONFIG_HOME=str(root/'config'),XDG_DATA_HOME=str(root/'data'),XDG_STATE_HOME=str(root/'state'),XDG_DATA_DIRS=str(root/'system'),XDG_CONFIG_DIRS=str(root/'system-config'))
        apps=root/'system/applications';apps.mkdir(parents=True)
        helper=root/'application fixture';shutil.copy2('/usr/bin/printf',helper)
        original=apps/'app-fixture.desktop'
        text=f'[Desktop Entry]\nType=Application\nName=Browser Fixture\nExec="{helper}" "%%s|%%s" %U tail\nIcon=web-browser\nMimeType=text/html;x-scheme-handler/https;\nDBusActivatable=true\n'
        original.write_text(text)
        config=root/'config/igpu-man/config.toml'
        startup=root/'config/autostart';startup.mkdir(parents=True)
        (startup/'unrelated.desktop').write_text('existing startup data')
        session=root/'config/ksmserverrc';session.write_text('[General]\nloginMode=restorePreviousLogout\nexcludeApps=existing\n')
        mime=root/'config/mimeapps.list';mime.write_text('[Default Applications]\nx-scheme-handler/https=another-browser.desktop;\n')
        before={p:p.read_bytes() for p in [session,mime,startup/'unrelated.desktop']}
        daemon=None
        def cli(*args,data=None,ok=True):
            r=subprocess.run([str(BIN/'igpu-man'),*args],env=env,input=data,text=True,capture_output=True,timeout=35)
            assert (r.returncode==0)==ok,(args,r.stderr)
            return r.stdout
        def rpc(method,payload,ok=True):
            out=cli('internal-request',method,data=json.dumps(payload),ok=ok)
            return json.loads(out) if ok else None
        def start():
            child=subprocess.Popen([str(BIN/'igpu-mand')],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            for _ in range(100):
                r=subprocess.run([str(BIN/'igpu-man'),'status','--json'],env=env,text=True,capture_output=True,timeout=5)
                if r.returncode==0:
                    require_collector(child)
                    return child,json.loads(r.stdout)
                assert child.poll() is None
                time.sleep(.05)
            child.terminate();child.wait(timeout=10);raise AssertionError('Fixture collector unavailable')
        def capture(stage):
            directory=os.environ.get('IGPU_MAN_CAPTURE_APPS_DIR')
            if directory:
                output=Path(directory)/f'apps-{stage}.png';output.parent.mkdir(parents=True,exist_ok=True)
                capture_env=dict(env,IGPU_MAN_SCREENSHOT_APP_HOME=stage,IGPU_MAN_SCREENSHOT_TO=str(output))
                subprocess.run([str(BIN/'igpu-man-desktop')],env=capture_env,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=25)
                assert output.exists(),'Build screenshot feature before captures'
        try:
            daemon,sample=start()
            assert rpc('profiles',{})==[]
            cli('profile','normal','x','--desktop-id','app-fixture.desktop','--login',ok=False)
            setup=dict(desktop_id='app-fixture.desktop',cli_path=str(BIN/'igpu-man'))
            rpc('app_route_preview',dict(setup,login=True),ok=False)
            if not any(g['integrated'] for g in sample['gpus']):
                rpc('app_route_preview',setup,ok=False)
                print('SKIP: successful application routing needs an AMD iGPU; input rejection passed')
                return
            capture('empty');capture('catalog')
            preview=rpc('app_route_preview',setup)
            assert rpc('profiles',{})==[] and not config.exists()
            capture('preview')
            original.write_text(text+'# changed\n')
            rpc('app_route_apply',{'setup':setup,'token':preview['token']},ok=False)
            assert not config.exists()
            original.write_text(text)
            preview=rpc('app_route_preview',setup)
            # No admission bypass during coordinated maintenance or active capture.
            token='a'*64;rpc('prepare_update',{'token':token})
            rpc('app_route_apply',{'setup':setup,'token':preview['token']},ok=False)
            rpc('cancel_update',{'token':token})
            recording=rpc('start',{'label':'existing recording'})
            rpc('app_route_apply',{'setup':setup,'token':preview['token']},ok=False)
            rpc('stop',{'expected_id':recording})
            saved_recordings=rpc('recordings',{})
            status=rpc('app_route_apply',{'setup':setup,'token':preview['token']})[0]
            assert status['launcher_ok'] and status['login_ok'] is None and status['restore_excluded'] is None
            profiles=rpc('profiles',{});assert len(profiles)==1 and profiles[0]['verification'] is None
            profile=status['profile']
            assert cli('launch','--normal',profile,'--','one argument')=='one argument|tail'
            assert all(p.read_bytes()==data for p,data in before.items())
            assert sorted(p.name for p in startup.iterdir())==['unrelated.desktop']
            assert original.read_text()==text
            # Main status must not claim verification for an exited printf process.
            assert not any(c.get('verification',{}).get('rendering')=='target_gpu' for c in rpc('candidates',{}) if c.get('verification') and c['launch_path'].get('registered_profile')==profile)
            capture('managed')
            daemon.terminate();daemon.wait(timeout=10);daemon,_=start()
            assert rpc('normal_launch_status',{})[0]['phase']=='active'
            rpc('normal_launch_undo',{'profile':profile})
            assert rpc('profiles',{})==[] and rpc('normal_launch_status',{})==[]
            assert rpc('recordings',{})==saved_recordings
            assert all(p.read_bytes()==data for p,data in before.items())
            # An existing compatible saved setup is reused and preserved by Undo.
            cli('profile','import','keep-existing','app-fixture.desktop','--gpu','igpu')
            preview=rpc('app_route_preview',setup);assert preview['profile']=='keep-existing'
            rpc('app_route_apply',{'setup':setup,'token':preview['token']})
            rpc('normal_launch_undo',{'profile':'keep-existing'})
            assert len(rpc('profiles',{}))==1
            print('PASS: app selection without profiles/tests, read-only preview, stale/maintenance/recording guards, direct argv, no startup/session/default-handler edits, persistence and Undo; saved setups/recordings preserved')
        finally:
            if daemon is not None and daemon.poll() is None:daemon.terminate();daemon.wait(timeout=10)

if __name__=='__main__':main()
