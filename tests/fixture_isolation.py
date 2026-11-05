#!/usr/bin/env python3
"""Occupied-private-bus refusals and wrong-peer checks; never use the real bus."""
from fixture_bus import require_empty, require_collector
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BIN = Path(os.environ.get('IGPU_MAN_BIN_DIR', ROOT / 'target/debug')).resolve()
FIXTURES = (
    'app_routing', 'normal_launches', 'placement_workflow', 'integration',
    'update_protocol', 'recording_responsiveness', 'recording_integrity',
    'security_boundaries', 'dashboard_lifecycle', 'desktop_smoke',
    'worker_recovery', 'measure_runtime', 'measure_threads', 'live_session_smoke',
    'tray_startup', 'tray_lifecycle',
)


def main():
    require_empty()
    with tempfile.TemporaryDirectory(prefix='sidecar-bus-isolation-') as temporary:
        root = Path(temporary)
        env = dict(os.environ, XDG_CONFIG_HOME=str(root / 'config'),
                   XDG_DATA_HOME=str(root / 'data'), XDG_STATE_HOME=str(root / 'state'))
        child = subprocess.Popen([str(BIN / 'igpu-mand')], env=env,
                                 stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        other = None
        try:
            def rpc(operation, value=None):
                result = subprocess.run([str(BIN / 'igpu-man'), 'internal-request', operation],
                    input=json.dumps(value or {}), env=env, text=True, capture_output=True,
                    check=True, timeout=8)
                return json.loads(result.stdout)

            for _ in range(100):
                assert child.poll() is None, 'Owned fixture collector exited'
                ready = subprocess.run([str(BIN / 'igpu-man'), 'status', '--json'],
                                       env=env, capture_output=True, timeout=5)
                if ready.returncode == 0:
                    require_collector(child)
                    break
                time.sleep(.05)
            else:
                raise AssertionError('Owned fixture collector not ready')
            recording = rpc('start', {'label':'fixture-must-not-stop'})
            def stable_catalog():
                # Sampling must continue; only sample counts may naturally grow.
                return [{key:value for key,value in row.items() if key != 'samples'} for row in rpc('recordings')]

            before = (rpc('profiles'), stable_catalog())
            for fixture in FIXTURES:
                arguments = [str(root / 'unused')] if fixture in ('desktop_smoke', 'worker_recovery', 'measure_runtime', 'measure_threads', 'live_session_smoke') else []
                result = subprocess.run([sys.executable, str(ROOT / 'tests' / f'{fixture}.py'), *arguments],
                                        env=env, text=True, capture_output=True, timeout=8)
                assert result.returncode != 0 and 'Refusing existing' in result.stderr, fixture
                assert child.poll() is None
                assert rpc('snapshot')['active_recording'] == recording, fixture
                assert (rpc('profiles'), stable_catalog()) == before, fixture
            # Correct readiness cannot be inferred from a response alone.
            other = subprocess.Popen(['/usr/bin/sleep', '30'])
            try:
                require_collector(other)
            except RuntimeError as error:
                assert 'expected bus name' in str(error)
            else:
                raise AssertionError('Unrelated live PID accepted as fixture collector')
            require_collector(child)
            rpc('stop', {'expected_id':recording})
            print(f'PASS: {len(FIXTURES)} fixtures refuse an occupied private bus; active recording/catalogs preserved; wrong peer rejected')
        finally:
            for process in (other, child):
                if process and process.poll() is None:
                    process.terminate()
                    process.wait(timeout=8)


if __name__ == '__main__':
    main()
