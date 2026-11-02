#!/usr/bin/env python3
"""Explicit, transactional developer update of a user-owned Sidecar install.

Never called by the desktop or package manager. No downloads or background work.
"""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import secrets
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time

ROOT=Path(__file__).resolve().parents[1]
BUS='io.github.igpu_man.Collector'
OBJECT='/io/github/igpu_man/Collector'
INTERFACE=BUS+'1'
NAMES=['igpu-man','igpu-mand','igpu-man-desktop']

class UpdateError(RuntimeError): pass

def run(args, *, timeout=35, check=True, env=None, cwd=None):
    p=subprocess.run(list(map(str,args)),capture_output=True,text=True,timeout=timeout,env=env,cwd=cwd)
    if check and p.returncode:raise UpdateError(f'{Path(str(args[0])).name} failed (exit {p.returncode}); inspect the update journal/systemd user journal')
    return p

def rpc(method,value=None):
    value=dict(value or {},_request_id=f'{os.getpid()}-{time.monotonic_ns()}-1')
    p=run(['busctl','--user','--json=short','call',BUS,OBJECT,INTERFACE,'Request','ss',method,json.dumps(value)],check=False)
    if p.returncode:raise UpdateError(f'Collector rejected {method}; no request payload was logged. Check recording/maintenance state.')
    return json.loads(json.loads(p.stdout)['data'][0])

def atomic(path,data,mode=0o600):
    path=Path(path);path.parent.mkdir(parents=True,exist_ok=True)
    fd,name=tempfile.mkstemp(prefix='.sidecar-update-',dir=path.parent)
    try:
        with os.fdopen(fd,'wb') as output:
            os.fchmod(output.fileno(),mode);output.write(data);output.flush();os.fsync(output.fileno())
        os.replace(name,path)
        directory=os.open(path.parent,os.O_DIRECTORY)
        try:os.fsync(directory)
        finally:os.close(directory)
    finally:Path(name).unlink(missing_ok=True)

def identity(pid):
    try:return {'pid':pid,'start_ticks':int(Path(f'/proc/{pid}/stat').read_text().rsplit(')',1)[1].split()[19])}
    except FileNotFoundError:return None

def sha(path):
    with Path(path).open('rb') as file:return hashlib.file_digest(file,'sha256').hexdigest()

def verify_manifest(directory):
    manifest=json.loads((directory/'build-info.json').read_text())
    for name in NAMES:
        if sha(directory/name)!=manifest['artifacts'][name]['sha256']:raise UpdateError('Release artifact hash mismatch')
    return manifest

def service(action):return run(['systemctl','--user',action,'igpu-man.service'],timeout=35)

def property_(key):return run(['systemctl','--user','show','igpu-man.service',f'--property={key}','--value']).stdout.strip()

def wait_ready():
    deadline=time.monotonic()+15
    while time.monotonic()<deadline:
        try:return rpc('snapshot')
        except UpdateError:time.sleep(.1)
    raise UpdateError('Collector did not become ready')

def gui_status(binary):return json.loads(run([binary,'--status']).stdout)

def close_gui(binary):
    # Independent of installed executables so interrupted/corrupt GUI updates can recover.
    args=['busctl','--user','--json=short','call','org.freedesktop.DBus','/org/freedesktop/DBus','org.freedesktop.DBus','NameHasOwner','s','io.github.igpu_man.Dashboard']
    def exists():return json.loads(run(args).stdout)['data'][0]
    if not exists():return
    run(['busctl','--user','call','io.github.igpu_man.Dashboard','/io/github/igpu_man/Dashboard','io.github.igpu_man.Dashboard1','Quit'])
    deadline=time.monotonic()+8
    while time.monotonic()<deadline:
        if not exists():return
        time.sleep(.05)
    raise UpdateError('Dashboard did not exit')

def start_gui(binary,mode):
    if mode is None:return
    # stdout/stderr never inherit user app output and remain private if logging fails.
    fd,log=tempfile.mkstemp(prefix='sidecar-update-ui-',suffix='.stderr')
    with os.fdopen(fd,'w') as output:
        subprocess.Popen([str(binary),*mode],cwd=Path.home(),stdin=subprocess.DEVNULL,stdout=output,stderr=output,start_new_session=True)
    deadline=time.monotonic()+15
    while time.monotonic()<deadline:
        s=gui_status(binary)
        if s.get('ready') and (s.get('tray_resident') if mode==['--tray'] else s.get('visible')):return
        time.sleep(.1)
    raise UpdateError('Dashboard did not recover')

class FileTransaction:
    def __init__(self,root):self.root=Path(root);self.entries=[]
    def apply(self,staged,prefix):
        staged=Path(staged);prefix=Path(prefix)
        for file in sorted(staged.rglob('*')):
            if not file.is_file():continue
            if file.is_symlink():raise UpdateError('Symlink in staged install')
            target=Path('/')/file.relative_to(staged)
            if not target.parent.resolve().is_relative_to(prefix.resolve()):raise UpdateError('Staged path outside user prefix')
            relative=target.relative_to(prefix)
            allowed={*(Path('bin')/name for name in NAMES),Path('share/systemd/user/igpu-man.service'),Path('share/applications/igpu-man.desktop'),Path('share/icons/hicolor/scalable/apps/igpu-man.svg'),Path('share/licenses/igpu-man/LICENSE')}
            if relative not in allowed and not relative.is_relative_to('share/doc/igpu-man'):raise UpdateError('Staged path is not an installed Sidecar asset')
            if target.is_symlink():raise UpdateError('Refusing to replace an installed symlink')
            backup=self.root/'files'/str(len(self.entries))
            existed=target.exists()
            if existed:
                backup.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(target,backup);backup.chmod(0o600)
            entry={'target':str(target),'backup':str(backup),'existed':existed,'mode':target.stat().st_mode&0o777 if existed else 0o644,'sha256':sha(backup) if existed else None}
            self.entries.append(entry)
            atomic(self.root/'files.json',json.dumps(self.entries,indent=2).encode())
            atomic(target,file.read_bytes(),file.stat().st_mode&0o777)
    def rollback(self):
        for entry in reversed(self.entries):
            target=Path(entry['target'])
            if entry['existed']:
                if sha(entry['backup'])!=entry['sha256']:raise UpdateError('Rollback backup hash mismatch')
                atomic(target,Path(entry['backup']).read_bytes(),entry['mode'])
            else:target.unlink(missing_ok=True)

def backup_data(destination,config,data):
    destination.mkdir(mode=0o700)
    if config.exists():atomic(destination/'config.toml',config.read_bytes())
    database=data/'recordings.sqlite'
    if database.exists():
        with sqlite3.connect(database.as_uri()+'?mode=ro',uri=True) as source:
            if source.execute('PRAGMA user_version').fetchone()[0]!=1:raise UpdateError('Unsupported storage schema; manual migration required')
            with sqlite3.connect(destination/'recordings.sqlite') as target:source.backup(target)
        (destination/'recordings.sqlite').chmod(0o600)


def recover(root,prefix):
    root=Path(root).resolve();journal=json.loads((root/'journal.json').read_text())
    if journal['prefix']!=str(prefix):raise UpdateError('Recovery prefix mismatch')
    if journal['phase'] in ['complete','rolled_back','preflight_failed']:raise UpdateError('This transaction does not need recovery')
    active=property_('ActiveState')=='active'
    if active and rpc('snapshot')['active_recording'] is not None:raise UpdateError('Recording active; recovery refused')
    runtime=Path(os.environ.get('XDG_RUNTIME_DIR',f'/run/user/{os.getuid()}'))/'systemd/user/igpu-man.service.d/90-sidecar-update.conf'
    if runtime.exists():
        content=runtime.read_text();token=content.strip().split('IGPU_MAN_UPDATE_TOKEN=')[-1]
    else:token=json.loads((root/'lease.json').read_text())['token']
    if journal['phase']=='staging' and not (root/'files.json').exists():
        if active:rpc('cancel_update',{'token':token})
        atomic(root/'journal.json',json.dumps(dict(journal,phase='preflight_failed'),indent=2).encode())
        print('Abandoned interrupted staging; installed programs and user data were not changed');return
    if active:rpc('prepare_update',{'token':token})
    launches=json.loads((root/'launches.json').read_text())['launches']
    tx=FileTransaction(root);tx.entries=json.loads((root/'files.json').read_text()) if (root/'files.json').exists() else []
    for entry in tx.entries:
        if not Path(entry['target']).parent.resolve().is_relative_to(prefix) or not Path(entry['backup']).resolve().is_relative_to(root/'files'):
            raise UpdateError('Unsafe recovery manifest')
    close_gui(prefix/'bin/igpu-man-desktop')
    service('stop');tx.rollback()
    atomic(runtime,f'[Service]\nEnvironment=IGPU_MAN_UPDATE_TOKEN={token}\n'.encode())
    run(['systemctl','--user','daemon-reload']);service('start');wait_ready()
    for launch in launches:
        if identity(launch['process']['pid'])==launch['process']:rpc('register_launch',dict(launch,_update_token=token))
    runtime.unlink();run(['systemctl','--user','daemon-reload']);rpc('cancel_update',{'token':token})
    atomic(root/'journal.json',json.dumps(dict(journal,phase='rolled_back'),indent=2).encode())
    start_gui(prefix/'bin/igpu-man-desktop',journal.get('gui_mode'))
    print(f'Restored the previous installed programs; user data was not overwritten. Backups: {root}')


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prefix',type=Path,default=Path.home()/'.local')
    parser.add_argument('--recover',type=Path,help='Restore programs from an interrupted transaction; user data is never overwritten')
    args=parser.parse_args();prefix=args.prefix.expanduser().resolve()
    if os.geteuid()==0 or prefix.is_symlink() or not prefix.is_relative_to(Path.home()):raise UpdateError('Only user-owned installs under your home are supported; use pacman for system packages')
    installed=prefix/'bin';doc=prefix/'share/doc/igpu-man';unit=prefix/'share/systemd/user/igpu-man.service'
    for name in NAMES:
        file=installed/name
        if not file.is_file() or file.is_symlink() or file.stat().st_uid!=os.getuid():raise UpdateError('Expected a user-owned installation')
    if Path(property_('FragmentPath'))!=unit or (not args.recover and property_('ActiveState')!='active'):raise UpdateError('Expected the active installed user unit; no service changes made')
    old_mode=None
    if not args.recover:
        pid=int(property_('MainPID'));before_id=identity(pid)
        if Path(os.readlink(f'/proc/{pid}/exe').removesuffix(' (deleted)'))!=installed/'igpu-mand':raise UpdateError('Unexpected collector owner')
        if rpc('snapshot')['active_recording'] is not None:raise UpdateError('Recording active: stop it before updating; nothing changed')
        if 'coordinated_updates' not in rpc('capabilities')['features']:raise UpdateError('One-time bootstrap required: the installed collector predates coordinated updates')
        old_gui=gui_status(installed/'igpu-man-desktop')
        if old_gui.get('visible'):raise UpdateError('Save editor drafts and close the dashboard to its tray before updating')
        old_mode=None if old_gui.get('running') is False else ['--tray']
    state=Path(os.environ.get('XDG_STATE_HOME',str(Path.home()/'.local/state')))/'igpu-man/updates'
    state.mkdir(parents=True,exist_ok=True,mode=0o700);state.chmod(0o700)
    lock=(state/'update.lock').open('a');os.chmod(lock.name,0o600)
    try:fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
    except BlockingIOError:raise UpdateError('Another updater is running') from None
    if args.recover:
        if not args.recover.resolve().is_relative_to(state.resolve()):raise UpdateError('Recovery directory must be in this account’s update state')
        recover(args.recover,prefix);return
    for journal in state.glob('run-*/journal.json'):
        if json.loads(journal.read_text()).get('phase') not in ['complete','rolled_back','preflight_failed']:
            raise UpdateError(f'Interrupted update requires review: {journal.parent}. Installed files and private backups were retained.')
    token=secrets.token_hex(32)
    runtime=Path(os.environ.get('XDG_RUNTIME_DIR',f'/run/user/{os.getuid()}'))/'systemd/user/igpu-man.service.d/90-sidecar-update.conf'
    if runtime.exists() or runtime.is_symlink():raise UpdateError('Existing runtime update override requires review')
    root=Path(tempfile.mkdtemp(prefix='run-',dir=state));root.chmod(0o700)
    def phase(name):atomic(root/'journal.json',json.dumps({'schema_version':1,'phase':name,'prefix':str(prefix),'gui_mode':old_mode},indent=2).encode());print(f'Sidecar update: {name}',flush=True)
    tx=FileTransaction(root);lease=False;stopped=False;gui_closed=False;committed=False
    phase('staging')
    atomic(root/'lease.json',json.dumps({'token':token}).encode())
    try:
        release=verify_manifest(ROOT/'target/release')
        stage=root/'stage'
        run(['make',f'PREFIX={prefix}',f'USER_UNIT_DIR={prefix}/share/systemd/user',f'DESTDIR={stage}','install'],timeout=90,cwd=ROOT)
        for name in NAMES:
            if sha(stage/installed.relative_to('/')/name)!=release['artifacts'][name]['sha256']:raise UpdateError('Staged executable hash mismatch')
        estimate=sum(p.stat().st_size for p in stage.rglob('*') if p.is_file())
        data=Path(os.environ.get('XDG_DATA_HOME',str(Path.home()/'.local/share')))/'igpu-man'
        config=Path(os.environ.get('XDG_CONFIG_HOME',str(Path.home()/'.config')))/'igpu-man/config.toml'
        estimate+=sum(p.stat().st_size for p in data.glob('recordings.sqlite*') if p.is_file())
        if shutil.disk_usage(state).free < estimate+64*1024*1024:raise UpdateError('Insufficient free space for rollback and data backup')
        lease=True
        registration=rpc('prepare_update',{'token':token})
        profiles=rpc('profiles');recordings=rpc('recordings')
        if registration['storage_schema']!=1 or registration['config_schema']!=1:raise UpdateError('Schema migration requires explicit review')
        atomic(root/'launches.json',json.dumps(registration).encode())
        phase('prepared')
        close_gui(installed/'igpu-man-desktop');gui_closed=True
        if identity(pid)!=before_id:raise UpdateError('Collector identity changed')
        # Same nonce survives service restart, closing the recording-start race.
        atomic(runtime,f'[Service]\nEnvironment=IGPU_MAN_UPDATE_TOKEN={token}\n'.encode())
        run(['systemctl','--user','daemon-reload'])
        stopped=True;service('stop')
        backup_data(root/'data-backup',config,data)
        phase('stopped')
        tx.apply(stage,prefix)
        phase('installed')
        run(['systemctl','--user','daemon-reload'])
        service('start');wait_ready()
        current=rpc('prepare_update',{'token':token})
        if any(current[key]!=registration[key] for key in ['storage_schema','config_schema']):
            committed=True
            raise UpdateError('Schema changed; preserve the new data and review private backups before any program downgrade')
        for launch in registration['launches']:
            if identity(launch['process']['pid'])==launch['process']:
                # Authorized restoration uses the same lease, never caller environment scraping.
                rpc('register_launch',dict(launch,_update_token=token))
        if rpc('profiles')!=profiles or rpc('recordings')!=recordings:raise UpdateError('Saved profile/recording catalog changed; inspect private backups')
        if rpc('capabilities')['build']['source_sha256']!=release['version_output'][1].split()[1]:raise UpdateError('Installed collector identity mismatch')
        runtime.unlink();run(['systemctl','--user','daemon-reload'])
        start_gui(installed/'igpu-man-desktop',old_mode)
        phase('verified')
        committed=True # Release may time out with uncertain outcome: never stop a newly started user recording.
        rpc('cancel_update',{'token':token});lease=False
        phase('complete')
        try:
            shutil.rmtree(stage)
            # Keep the last two successful backup sets; never prune incomplete work.
            completed=sorted((p for p in state.glob('run-*') if (p/'journal.json').exists() and json.loads((p/'journal.json').read_text()).get('phase')=='complete'),key=lambda p:p.stat().st_mtime)
            for older in completed[:-2]:shutil.rmtree(older)
        except OSError:print('Update committed; old backup/stage cleanup needs attention',file=sys.stderr)
        print(f'Updated Sidecar. Private rollback/data backup: {root}')
    except BaseException:
        if committed:
            phase('recovery_required')
            raise
        if stopped:
            try:
                if property_('ActiveState')=='active':rpc('prepare_update',{'token':token})
                service('stop')
                tx.rollback()
                atomic(runtime,f'[Service]\nEnvironment=IGPU_MAN_UPDATE_TOKEN={token}\n'.encode());run(['systemctl','--user','daemon-reload'])
                service('start');wait_ready()
                for launch in registration['launches']:
                    if identity(launch['process']['pid'])==launch['process']:rpc('register_launch',dict(launch,_update_token=token))
                runtime.unlink(missing_ok=True);run(['systemctl','--user','daemon-reload']);rpc('cancel_update',{'token':token})
                start_gui(installed/'igpu-man-desktop',old_mode)
                phase('rolled_back')
            except BaseException:
                phase('recovery_required');print(f'Rollback incomplete; retained programs and data backups at {root}',file=sys.stderr)
        else:
            runtime.unlink(missing_ok=True)
            if lease:
                try:rpc('cancel_update',{'token':token})
                except UpdateError:pass
            if gui_closed:start_gui(installed/'igpu-man-desktop',old_mode)
            phase('preflight_failed')
        raise

if __name__=='__main__':
    try:main()
    except subprocess.TimeoutExpired:print('Sidecar update timed out; inspect the private transaction journal before retrying',file=sys.stderr);sys.exit(1)
    except UpdateError as error:print(f'Sidecar update failed: {error}',file=sys.stderr);sys.exit(1)
