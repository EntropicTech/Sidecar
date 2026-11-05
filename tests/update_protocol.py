#!/usr/bin/env python3
"""Private-bus maintenance protocol: recording exclusion, nonce and boot lease."""
from fixture_bus import require_empty, require_collector
import json,os,secrets,subprocess,tempfile,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];BIN=ROOT/'target/debug'
def main():
 require_empty()
 with tempfile.TemporaryDirectory(prefix='sidecar-lease-') as temporary:
  env=dict(os.environ,XDG_CONFIG_HOME=temporary+'/config',XDG_DATA_HOME=temporary+'/data',XDG_STATE_HOME=temporary+'/state')
  child=None
  def rpc(op,value=None,ok=True):
   p=subprocess.run([str(BIN/'igpu-man'),'internal-request',op],env=env,input=json.dumps(value or {}),capture_output=True,text=True,timeout=5)
   assert (p.returncode==0)==ok,(op,p.stderr)
   return json.loads(p.stdout) if ok else None
  def start(token=None):
   e=dict(env)
   if token:e['IGPU_MAN_UPDATE_TOKEN']=token
   p=subprocess.Popen([str(BIN/'igpu-mand')],env=e,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
   for _ in range(100):
    r=subprocess.run([str(BIN/'igpu-man'),'status','--json'],env=env,capture_output=True,timeout=5)
    if r.returncode==0:
     require_collector(p)
     return p
    assert p.poll() is None
    time.sleep(.04)
   raise AssertionError('Collector not ready')
  try:
   child=start();token=secrets.token_hex(32);other=secrets.token_hex(32)
   record=rpc('start',{'label':'do-not-interrupt'})
   rpc('prepare_update',{'token':token},ok=False)
   assert rpc('snapshot')['active_recording']==record
   rpc('stop',{'expected_id':record})
   rpc('prepare_update',{'token':'invalid'},ok=False)
   lease=rpc('prepare_update',{'token':token});assert lease['storage_schema']==1
   before=rpc('snapshot')['monotonic_ns'];time.sleep(1.1)
   assert rpc('snapshot')['monotonic_ns']>before
   for op,value in [('start',{}),('mark',{}),('save_profile',{}),('create_launcher',{}),('register_launch',{}),('delete_recording',{'id':record})]:rpc(op,value,ok=False)
   rpc('cancel_update',{'token':other},ok=False);rpc('prepare_update',{'token':other},ok=False)
   rpc('cancel_update',{'token':token})
   r=rpc('start',{'label':'lease-released'});rpc('stop',{'expected_id':r})
   child.terminate();child.wait(timeout=10);child=start(token)
   rpc('start',{},ok=False);rpc('cancel_update',{'token':token})
   r=rpc('start',{'label':'boot-lease-released'});rpc('stop',{'expected_id':r})
   print('PASS: active recording untouched, malformed/wrong nonce rejected, mutations excluded while sampling continues, release and restart-carried lease work')
  finally:
   if child and child.poll() is None:child.terminate();child.wait(timeout=10)
if __name__=='__main__':main()
