#!/usr/bin/env python3
"""Synthetic large-recording read vs live sampling; isolated bus/XDG only.

IGPU_MAN_BIN_DIR selects a before/after build; --baseline reports rather than
asserting the expected older lock stall. --native checks dashboard telemetry too.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import json
import hashlib
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import time

ROOT=Path(__file__).resolve().parents[1]
BIN=Path(os.environ.get('IGPU_MAN_BIN_DIR',ROOT/'target/debug'))

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--baseline',action='store_true')
    parser.add_argument('--native',action='store_true')
    parser.add_argument('--report',type=Path)
    parser.add_argument('--mib',type=int,default=128)
    parser.add_argument('--seconds',type=float,default=2)
    args=parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='igpu-man-responsive-') as temporary:
        root=Path(temporary)
        env=dict(os.environ,XDG_CONFIG_HOME=str(root/'config'),XDG_DATA_HOME=str(root/'data'),XDG_STATE_HOME=str(root/'state'))
        daemon=None;desktop=None
        def cli(*options,check=True,timeout=35):
            p=subprocess.run([str(BIN/'igpu-man'),*map(str,options)],env=env,text=True,capture_output=True,timeout=timeout)
            if check:assert p.returncode==0,p.stderr
            return p
        def start():
            p=subprocess.Popen([str(BIN/'igpu-mand')],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            for _ in range(100):
                result=cli('status','--json',check=False,timeout=5)
                if result.returncode==0:return p,json.loads(result.stdout)
                assert p.poll() is None,'Test collector exited'
                time.sleep(.05)
            raise AssertionError('Test collector unavailable')
        try:
            daemon,snapshot=start()
            large=json.loads(cli('record','start','synthetic-large-read').stdout)
            cli('record','stop');daemon.terminate();daemon.wait(timeout=5);daemon=None
            snapshot['diagnostics']['kwin_support']='synthetic padding '+('x'*32768)
            snapshot['applications']=[]
            raw=json.dumps(snapshot,separators=(',',':'))
            count=max(1,args.mib*1024*1024//len(raw))
            with sqlite3.connect(root/'data/igpu-man/recordings.sqlite') as db:
                db.execute('DELETE FROM samples WHERE recording_id=?',(large,))
                db.executemany('INSERT INTO samples(recording_id,monotonic_ns,json) VALUES(?,?,?)',((large,i,raw) for i in range(count)))
            daemon,_=start()
            recording=json.loads(cli('record','start','responsive-live-capture').stdout)
            if args.native:
                capture_env=dict(env,IGPU_MAN_SCREENSHOT_RECORDING_ID=str(large),IGPU_MAN_SCREENSHOT_TO=str(root/'dashboard.png'),IGPU_MAN_SCREENSHOT_WAIT_MS='10000')
                desktop=subprocess.Popen([str(BIN/'igpu-man-desktop')],env=capture_env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
                for _ in range(100):
                    p=subprocess.run([str(BIN/'igpu-man-desktop'),'--status'],env=env,text=True,capture_output=True,timeout=3)
                    status=json.loads(p.stdout)
                    if status.get('ready'):break
                    assert desktop.poll() is None
                    time.sleep(.02)
            latencies=[];timestamps=[];errors=0;ui_times=[]
            with ThreadPoolExecutor(max_workers=2) as pool:
                started=time.monotonic()
                def read_repeatedly():
                    requests=0
                    while time.monotonic()-started<args.seconds:
                        cli('record','show',large);requests+=1
                    return requests
                reads=[pool.submit(read_repeatedly) for _ in range(1 if args.native else 2)]
                while not all(f.done() for f in reads):
                    before=time.monotonic();response=cli('status','--json',check=False,timeout=5)
                    latencies.append(time.monotonic()-before)
                    if response.returncode==0:timestamps.append(json.loads(response.stdout)['monotonic_ns'])
                    else:errors+=1
                    if desktop and desktop.poll() is None:
                        state=json.loads(subprocess.check_output([str(BIN/'igpu-man-desktop'),'--status'],env=env,text=True,timeout=3))
                        ui_times.append(state.get('telemetry_monotonic_ns',0))
                    time.sleep(.08)
                requests=sum(future.result() for future in reads)
                elapsed=time.monotonic()-started
            stop_seconds=None
            if not args.baseline:
                with ThreadPoolExecutor(max_workers=1) as pool:
                    export=root/'large-export';future=pool.submit(cli,'record','export',large,export)
                    for _ in range(1000):
                        if export.exists() or future.done():break
                        time.sleep(.001)
                    before=time.monotonic();cli('record','stop','--expected-id',recording);stop_seconds=time.monotonic()-before
                    future.result();assert (export/'telemetry.jsonl').stat().st_size>args.mib*1024*1024*.95
            else:cli('record','stop')
            detail=json.loads(cli('record','show',recording).stdout)
            report={'synthetic_recording_mib':round(count*len(raw)/1024/1024,2),'sample_rows':count,'concurrent_readers':2,'completed_read_requests':requests,'read_seconds':elapsed,'snapshot_calls':len(latencies),'snapshot_errors':errors,'stop_during_export_seconds':stop_seconds,'max_snapshot_seconds':max(latencies),'distinct_live_samples':len(set(timestamps)),'persisted_live_samples':detail['recording']['samples'],'dashboard_telemetry_updates':len(set(t for t in ui_times if t)),'baseline':args.baseline,'collector_sha256':hashlib.file_digest((BIN/'igpu-mand').open('rb'),'sha256').hexdigest(),'limits':'Synthetic recording, real isolated collector; timings include CLI startup. Not a gaming or idle-overhead measurement.'}
            print(json.dumps(report,indent=2),flush=True)
            if args.report:args.report.write_text(json.dumps(report,indent=2)+'\n')
            if not args.baseline:
                assert elapsed>=.5,'Fixture too small to exercise concurrent sampling'
                assert errors==0 and max(latencies)<.75 and stop_seconds<.75,report
                assert len(set(timestamps))>=3 and detail['recording']['samples']>=3,report
                if args.native:assert len(set(t for t in ui_times if t))>=2,report
                print('PASS: large reads leave live snapshots/sampling/persistence responsive')
        finally:
            if desktop and desktop.poll() is None:desktop.terminate();desktop.wait(timeout=5)
            if daemon and daemon.poll() is None:daemon.terminate();daemon.wait(timeout=10)
if __name__=='__main__':main()
