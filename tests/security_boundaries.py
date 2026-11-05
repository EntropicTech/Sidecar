#!/usr/bin/env python3
"""Security regressions using synthetic values and owned processes on a private bus.

Run: dbus-run-session -- python3 tests/security_boundaries.py
No installed service, browser, launcher or user configuration is modified.
"""
from fixture_bus import require_empty, require_collector
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BIN = Path(os.environ.get('IGPU_MAN_BIN_DIR', ROOT / 'target/debug'))
BUS = 'io.github.igpu_man.Collector'


def main():
    require_empty()
    with tempfile.TemporaryDirectory(prefix='sidecar-security-') as temporary:
        root = Path(temporary)
        env = dict(os.environ, XDG_CONFIG_HOME=str(root/'config'),
            XDG_DATA_HOME=str(root/'data'), XDG_STATE_HOME=str(root/'state'),
            XDG_DATA_DIRS=str(root/'system'))
        daemon = child = None
        paused = False

        def cli(*args, payload=None, success=True):
            result = subprocess.run([str(BIN/'igpu-man'), *args], env=env,
                input=payload, text=True, capture_output=True, timeout=10)
            assert (result.returncode == 0) == success, (args, result.stderr)
            return result

        def rpc(operation, payload, success=True):
            # Payloads intentionally use the same private stdin bridge as updates.
            return cli('internal-request', operation, payload=json.dumps(payload), success=success)

        def start():
            process = subprocess.Popen([str(BIN/'igpu-mand')], env=env,
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            for _ in range(100):
                result = subprocess.run([str(BIN/'igpu-man'), 'status', '--json'],
                    env=env, capture_output=True, text=True, timeout=5)
                if result.returncode == 0:
                    require_collector(process)
                    return process, json.loads(result.stdout)
                assert process.poll() is None, 'Owned fixture collector exited'
                time.sleep(.05)
            process.terminate(); process.wait(timeout=5)
            raise AssertionError('Owned collector did not become ready')

        try:
            daemon, snapshot = start()
            profile = {'name':'trial-private', 'executable':'/bin/true', 'gpu':'igpu',
                'environment':{'SIDECAR_FIXTURE_VALUE':'synthetic-private-value'}}
            malicious = dict(profile, pending_launcher={'schema_version':1,
                'content':'[Desktop Entry]\nType=Application\nName=Synthetic fixture\nExec=/bin/true\n'})
            assert 'internal recovery state' in rpc('save_profile', malicious, success=False).stderr
            registration = {'process':{'pid':os.getpid(), 'start_ticks':int(
                Path('/proc/self/stat').read_text().rsplit(')',1)[1].split()[19])},
                'gpu':'igpu', 'anchor_ns':1, 'profile':malicious}
            assert 'internal recovery state' in rpc('register_launch', registration, success=False).stderr
            assert json.loads(cli('profile','list').stdout) == []
            assert not (root/'config/igpu-man/config.toml').exists()
            assert 'internal recovery state' in cli('try-profile',payload=json.dumps(malicious),success=False).stderr
            cli('try-profile',payload='{',success=False)
            cli('try-profile',payload='x'*(1024*1024+1),success=False)
            cli('try-profile','{"legacy":"argv-payload"}',success=False)
            for _ in range(12):
                for invalid in ['[]', '{']:
                    rejected = subprocess.run(['dbus-send','--session','--print-reply',
                        f'--dest={BUS}','/io/github/igpu_man/Collector',
                        'io.github.igpu_man.Collector1.Request','string:snapshot',f'string:{invalid}'],
                        env=env, text=True, capture_output=True, timeout=5)
                    assert rejected.returncode != 0
                rpc('snapshot', {})  # Parse failures release admission immediately.
            # Make the wrapper wait for an RPC, then inspect only its owned argv.
            daemon.send_signal(signal.SIGSTOP); paused = True
            child = subprocess.Popen([str(BIN/'igpu-man'),'try-profile'], env=env,
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            child.stdin.write(json.dumps(profile).encode()); child.stdin.close()
            time.sleep(.2)
            assert child.poll() is None
            argv = Path(f'/proc/{child.pid}/cmdline').read_bytes().split(b'\0')
            assert argv[1:2] == [b'try-profile'] and len(argv) == 3, argv
            assert b'synthetic-private-value' not in b' '.join(argv)
            child.terminate(); child.wait(timeout=5); child = None
            daemon.send_signal(signal.SIGCONT); paused = False
            daemon.terminate(); daemon.wait(timeout=10); daemon = None
            daemon, snapshot = start()
            assert json.loads(cli('profile','list').stdout) == []
            assert not list((root/'data/applications').glob('*.desktop'))
            if snapshot['gpus']:
                # Executable identity is unique; no existing application is reused.
                helper = root/'trial-helper'
                shutil.copy2('/usr/bin/printf', helper)
                profile.update(executable=str(helper), gpu=snapshot['gpus'][0]['pci'],
                    args=['%s|%s|%s','one argument','$(touch should-not-run)','"quoted"'])
                result = cli('try-profile',payload=json.dumps(profile))
                assert result.stdout == 'one argument|$(touch should-not-run)|"quoted"'
                assert not (ROOT/'should-not-run').exists()
                shutil.copy2('/usr/bin/printenv', helper)
                profile['args'] = ['SIDECAR_FIXTURE_VALUE']
                assert cli('try-profile',payload=json.dumps(profile)).stdout.strip() == 'synthetic-private-value'
            else:
                print('SKIP: GPU-backed trial argv preservation needs an AMD GPU')
            print('PASS: private trial transport, bounded/invalid input, recovery-input rejection, restart safety, parse-permit release and direct argv preservation')
        finally:
            if child is not None and child.poll() is None:
                child.terminate(); child.wait(timeout=5)
            if daemon is not None and daemon.poll() is None:
                if paused: daemon.send_signal(signal.SIGCONT)
                daemon.terminate(); daemon.wait(timeout=10)


if __name__ == '__main__':
    main()
