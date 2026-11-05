#!/usr/bin/env python3
"""Private-bus recording comparisons, note writes and abrupt export interruption.

Run after cargo build: dbus-run-session -- python3 tests/recording_integrity.py
Only an owned fixture collector receives file-size limits/signals; no user service
or application is stopped. No GPU or display is required.
"""
from fixture_bus import require_empty, require_collector
import json
import os
from pathlib import Path
import resource
import signal
import sqlite3
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BIN = Path(os.environ.get('IGPU_MAN_BIN_DIR', ROOT/'target/debug'))


def main():
    require_empty()
    with tempfile.TemporaryDirectory(prefix='sidecar-recording-integrity-') as temporary:
        root = Path(temporary)
        env = dict(os.environ, XDG_CONFIG_HOME=str(root/'config'),
            XDG_DATA_HOME=str(root/'data'), XDG_STATE_HOME=str(root/'state'))
        daemon = None

        def cli(*args, success=True):
            result = subprocess.run([str(BIN/'igpu-man'),*map(str,args)],env=env,
                text=True,capture_output=True,timeout=30)
            assert (result.returncode == 0) == success, (args,result.stderr)
            return json.loads(result.stdout) if success else result

        def start():
            child = subprocess.Popen([str(BIN/'igpu-mand')],env=env,
                stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            for _ in range(100):
                result = subprocess.run([str(BIN/'igpu-man'),'status','--json'],
                    env=env,text=True,capture_output=True,timeout=5)
                if result.returncode == 0:
                    require_collector(child)
                    return child,json.loads(result.stdout)
                assert child.poll() is None, 'Owned fixture collector exited'
                time.sleep(.05)
            child.terminate();child.wait(timeout=5)
            raise AssertionError('Owned collector did not become ready')

        try:
            daemon,snapshot = start()
            ids = []
            for _ in range(2):
                id = cli('record','start','condition')
                cli('record','stop')
                cli('record','import',id,ROOT/'tests/fixtures/mangohud.csv')
                ids.append(id)
            a,b = ids
            cli('record','notes',a,'A notes')
            cli('record','notes',b,'B notes')
            assert cli('record','show',a)['recording']['notes'] == 'A notes'
            assert cli('record','show',b)['recording']['notes'] == 'B notes'
            assert 'distinct recording IDs' in cli('record','compare',a,a,success=False).stderr
            assert cli('record','compare',a,b)['run_variation_by_label']['condition']['runs'] == 2
            daemon.terminate();daemon.wait(timeout=10);daemon = None
            database = root/'data/igpu-man/recordings.sqlite'
            # Synthetic conditions isolate the comparison from changing desktop diagnostics.
            with sqlite3.connect(database) as db:
                for index,id in enumerate(ids):
                    profile = {'name':'fixture','executable':'/bin/true',
                        'gpu':'pci-a' if index == 0 else 'pci-b',
                        'args':['--quality=low' if index == 0 else '--quality=high']}
                    metadata = {'environment':{'fixture':'same hardware'},
                        'normal_interval_ms':1000,'recording_interval_ms':250 if index == 0 else 500,
                        'stutter_threshold_ms':25,'profiles':[profile],
                        'launches':[{'gpu':profile['gpu'],'profile':profile}]}
                    db.execute('UPDATE recordings SET metadata=? WHERE id=?',(json.dumps(metadata),id))
                snapshot['applications'] = []
                snapshot['diagnostics']['kwin_support'] = 'synthetic padding '+('x'*65536)
                raw = json.dumps(snapshot,separators=(',',':'))
                db.execute('DELETE FROM samples WHERE recording_id=?',(a,))
                db.executemany('INSERT INTO samples(recording_id,monotonic_ns,json) VALUES(?,?,?)',
                    ((a,i,raw) for i in range(256)))
            daemon,_ = start()
            warnings = ' '.join(cli('record','compare',a,b)['mismatch_warnings'])
            assert 'sampling intervals' in warnings and 'effective launch settings' in warnings
            # SIGXFSZ deterministically interrupts a write, without racing a fast
            # filesystem or requiring test hooks in production. Bound core dumps.
            resource.prlimit(daemon.pid,resource.RLIMIT_CORE,(0,0))
            resource.prlimit(daemon.pid,resource.RLIMIT_FSIZE,(1024*1024,1024*1024))
            interrupted = root/'interrupted-export'
            cli('record','export',a,interrupted,success=False)
            assert daemon.wait(timeout=10) == -signal.SIGXFSZ, 'Expected abrupt owned-helper interruption'
            daemon = None
            assert (interrupted/'metadata.json').exists()
            assert (interrupted/'INCOMPLETE').is_file()
            assert 0 < (interrupted/'telemetry.jsonl').stat().st_size < (len(raw)+1)*256
            assert (interrupted/'INCOMPLETE').stat().st_mode & 0o777 == 0o600
            daemon,_ = start()
            cli('record','export',a,interrupted,success=False)  # Preserve failed output.
            complete = root/'complete-export'
            cli('record','export',a,complete)
            assert not (complete/'INCOMPLETE').exists()
            assert (complete/'telemetry.jsonl').stat().st_size == (len(raw)+1)*256
            assert (complete/'frames-0.csv').read_bytes() == (ROOT/'tests/fixtures/mangohud.csv').read_bytes()
            assert complete.stat().st_mode & 0o777 == 0o700
            if output := os.environ.get('IGPU_MAN_RECORDING_SCREENSHOT_TO'):
                capture = dict(env, IGPU_MAN_SCREENSHOT_TAB='3',
                    IGPU_MAN_SCREENSHOT_RECORDING_ID=str(a),IGPU_MAN_SCREENSHOT_TO=output,
                    IGPU_MAN_SCREENSHOT_WAIT_MS='1200')
                subprocess.run([str(BIN/'igpu-man-desktop')],env=capture,
                    stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,check=True,timeout=30)
                assert Path(output).is_file(), 'Build the screenshot feature for native captures'
            print('PASS: isolated note writes, distinct comparison runs/settings warnings, abrupt export interruption marked before writing, preserved failed destination, complete retry after collector restart')
        finally:
            if daemon is not None and daemon.poll() is None:
                daemon.terminate();daemon.wait(timeout=10)


if __name__ == '__main__':
    main()
