#!/usr/bin/env python3
"""Cold --tray: no graphics/display needed, delayed host, idempotency and fallback."""
import json,os,subprocess,tempfile,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
BIN=Path(os.environ.get('IGPU_MAN_BIN_DIR',ROOT/'target/debug'))
def main():
 with tempfile.TemporaryDirectory(prefix='sidecar-startup-') as temporary:
  env={k:v for k,v in os.environ.items() if not k.startswith('IGPU_MAN_SCREENSHOT_')}
  env.update(XDG_CONFIG_HOME=temporary+'/config',XDG_DATA_HOME=temporary+'/data',XDG_STATE_HOME=temporary+'/state')
  child=None;watcher=None
  def gui(*args):return subprocess.check_output([str(BIN/'igpu-man-desktop'),*args],env=env,text=True,timeout=5)
  def status():return json.loads(gui('--status'))
  def wait(predicate,seconds=8):
   deadline=time.monotonic()+seconds
   while time.monotonic()<deadline:
    s=status()
    if predicate(s):return s
    assert child.poll() is None, (child.returncode, Path(temporary+'/stderr').read_text()[-4000:], [(p.name,p.read_text()[-6000:]) for p in Path(temporary+'/state/igpu-man/logs').glob('desktop*.jsonl')])
    time.sleep(.04)
   raise AssertionError(s)
  def host():
   p=subprocess.Popen([str(ROOT/'target/debug/examples/tray_fixture')],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL);time.sleep(.2);return p
  def start(no_display=False):
   launch=dict(env,DISPLAY='',WAYLAND_DISPLAY='') if no_display else env
   with open(temporary+'/stderr','w') as output:return subprocess.Popen([str(BIN/'igpu-man-desktop'),'--tray'],env=launch,stdout=output,stderr=output)
  def quit():
   gui('--quit');assert child.wait(timeout=5)==0
   assert status().get('running') is False
  try:
   assert status().get('running') is False
   watcher=host();child=start(no_display=True)
   s=wait(lambda s:s.get('tray_resident'))
   assert s['windows_created']==0 and s['updates']==0
   for _ in range(3):gui('--tray')
   time.sleep(.3);s=status();assert s['pid']==child.pid and s['windows_created']==0 and not s['visible']
   quit();watcher.terminate();watcher.wait(timeout=3);watcher=None
   child=start();s=wait(lambda s:s.get('ready'))
   assert s['windows_created']==0 and not s['visible']
   time.sleep(.6);watcher=host()
   wait(lambda s:s.get('tray_resident') and s.get('windows_created')==0)
   gui();s=wait(lambda s:s.get('visible') and s.get('windows_created')==1)
   gui('--tray');time.sleep(.3);s=status();assert s['visible'] and s['windows_created']==1
   gui('--close');wait(lambda s:s.get('tray_resident'));gui('--tray');assert status()['windows_created']==1
   quit();watcher.terminate();watcher.wait(timeout=3);watcher=None
   child=start();s=wait(lambda s:s.get('visible'),seconds=20)
   assert s['windows_created']==1 and not s['tray_available']
   quit()
   print('PASS: cold tray starts without display/GL/window, delayed host registers, duplicate startup preserves visibility, ordinary launch opens once, missing-host grace opens fallback, Quit works before first window')
  finally:
   if child and child.poll() is None:child.terminate();child.wait(timeout=5)
   if watcher and watcher.poll() is None:watcher.terminate();watcher.wait(timeout=5)
if __name__=='__main__':main()
