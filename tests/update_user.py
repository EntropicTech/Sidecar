#!/usr/bin/env python3
"""Transactional developer updater tests with a real private collector, fake systemd."""
import importlib.util,json,os,shutil,subprocess,tempfile,time
from pathlib import Path
from unittest.mock import patch
ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('update_user',ROOT/'packaging/update_user.py');u=importlib.util.module_from_spec(spec);spec.loader.exec_module(u)

def main():
 with tempfile.TemporaryDirectory(prefix='sidecar-updater-') as temporary:
  root=Path(temporary);home=root/'home';home.mkdir();prefix=home/'.local';runtime=root/'runtime';runtime.mkdir(mode=0o700)
  environment=dict(os.environ,XDG_CONFIG_HOME=str(root/'config'),XDG_DATA_HOME=str(root/'data'),XDG_STATE_HOME=str(root/'state'),XDG_RUNTIME_DIR=str(runtime))
  subprocess.run(['make',f'PREFIX={prefix}',f'USER_UNIT_DIR={prefix}/share/systemd/user','install'],cwd=ROOT,check=True,capture_output=True)
  cfg=root/'config/igpu-man/config.toml';cfg.parent.mkdir(parents=True);cfg.write_text('schema_version=1\n[[profiles]]\nname="preserve-me"\nexecutable="/usr/bin/true"\ngpu="0000:00:02.0"\n')
  autostart=root/'config/autostart/igpu-man.desktop';autostart.parent.mkdir();autostart.write_text('user-owned autostart unchanged\n')
  original_cfg=cfg.read_bytes();original_autostart=autostart.read_bytes()
  original_run=u.run;child=None
  class Manager:
   starts=0;fail_next=False
   def service(self,action):
    nonlocal child
    if action=='stop':
     if child and child.poll() is None:child.terminate();child.wait(timeout=10)
    elif action=='start':
     self.starts+=1
     if self.fail_next:self.fail_next=False;raise u.UpdateError('synthetic restart failure')
     env=dict(environment);override=runtime/'systemd/user/igpu-man.service.d/90-sidecar-update.conf'
     if override.exists():env['IGPU_MAN_UPDATE_TOKEN']=override.read_text().strip().split('IGPU_MAN_UPDATE_TOKEN=')[-1]
     child=subprocess.Popen([str(prefix/'bin/igpu-mand')],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
     u.wait_ready()
   def property(self,key):
    return {'FragmentPath':str(prefix/'share/systemd/user/igpu-man.service'),'ActiveState':'active' if child and child.poll() is None else 'inactive','MainPID':str(child.pid if child else 0)}[key]
  manager=Manager()
  def runner(args,**kwargs):
   if str(args[0])=='systemctl':return subprocess.CompletedProcess(args,0,'','')
   return original_run(args,**kwargs)
  try:
   with patch.dict(os.environ,environment),patch.object(Path,'home',return_value=home),patch.object(u,'run',side_effect=runner),patch.object(u,'service',side_effect=manager.service),patch.object(u,'property_',side_effect=manager.property):
    manager.service('start')
    record=u.rpc('start',{'label':'saved-recording'});u.rpc('stop',{'expected_id':record})
    profiles=u.rpc('profiles');records=u.rpc('recordings')
    with patch('sys.argv',['update_user','--prefix',str(prefix)]):u.main()
    assert cfg.read_bytes()==original_cfg and autostart.read_bytes()==original_autostart
    assert u.rpc('profiles')==profiles and u.rpc('recordings')==records
    assert child.poll() is None
    # Updating during capture must not stop, replace or change the active recording.
    active=u.rpc('start',{'label':'must-not-interrupt'});pid=child.pid
    with patch('sys.argv',['update_user','--prefix',str(prefix)]):
     try:u.main();raise AssertionError('Active update accepted')
     except u.UpdateError:pass
    assert child.pid==pid and u.rpc('snapshot')['active_recording']==active
    u.rpc('stop',{'expected_id':active})
    hashes={n:u.sha(prefix/'bin'/n) for n in u.NAMES}
    manager.fail_next=True
    with patch('sys.argv',['update_user','--prefix',str(prefix)]):
     try:u.main();raise AssertionError('Injected failure not reached')
     except u.UpdateError:pass
    assert child.poll() is None and all(u.sha(prefix/'bin'/n)==h for n,h in hashes.items())
    assert cfg.read_bytes()==original_cfg and autostart.read_bytes()==original_autostart
    journals=[json.loads(p.read_text())['phase'] for p in (root/'state/igpu-man/updates').glob('run-*/journal.json')]
    assert 'complete' in journals and 'rolled_back' in journals
    # Interrupted deployment with an inactive service can be recovered explicitly.
    state=root/'state/igpu-man/updates';interrupted=state/'run-interrupted';interrupted.mkdir(mode=0o700)
    token='a'*64;u.rpc('prepare_update',{'token':token});manager.service('stop')
    stage2=root/'interrupted-stage';file=stage2/(prefix/'bin/igpu-man').relative_to('/');file.parent.mkdir(parents=True);file.write_bytes(b'partial update');file.chmod(0o755)
    transaction=u.FileTransaction(interrupted);transaction.apply(stage2,prefix)
    u.atomic(interrupted/'journal.json',json.dumps({'prefix':str(prefix),'phase':'installed','gui_mode':None}).encode())
    u.atomic(interrupted/'launches.json',b'{"launches": []}')
    u.atomic(interrupted/'lease.json',json.dumps({'token':token}).encode())
    with patch('sys.argv',['update_user','--prefix',str(prefix),'--recover',str(interrupted)]):u.main()
    assert child.poll() is None and all(u.sha(prefix/'bin'/n)==h for n,h in hashes.items())
    # File transaction rejects data paths and restores prior permissions/content.
    stage=root/'file-stage';target=prefix/'bin/igpu-man';staged=stage/target.relative_to('/');staged.parent.mkdir(parents=True);staged.write_bytes(b'replacement');staged.chmod(0o755)
    tx=u.FileTransaction(root/'file-backup');before=target.read_bytes();tx.apply(stage,prefix);assert target.read_bytes()==b'replacement';tx.rollback();assert target.read_bytes()==before
    bad=stage/(prefix/'share/igpu-man/recordings.sqlite').relative_to('/');bad.parent.mkdir(parents=True);bad.write_text('do not install data')
    try:u.FileTransaction(root/'bad').apply(stage,prefix);raise AssertionError('Data replacement accepted')
    except u.UpdateError:pass
    print('PASS: real-collector update preserves config/records/autostart, active capture refused, restart failure rolls programs back, data backup/transaction bounds hold')
  finally:
   if child and child.poll() is None:child.terminate();child.wait(timeout=10)
if __name__=='__main__':main()
