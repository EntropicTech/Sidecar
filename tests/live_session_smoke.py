#!/usr/bin/env python3
"""Smoke-test the real Plasma tray/KWin session without installing anything.

Build the desktop with --features screenshot, then run:
python3 tests/live_session_smoke.py /tmp/igpu-man-live.png
Refuses to run if a collector already owns the session-bus name.
"""
from fixture_bus import COLLECTOR, DASHBOARD, require_unowned, require_collector
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / "target/debug"


def main():
    require_unowned(COLLECTOR, DASHBOARD)
    with tempfile.TemporaryDirectory(prefix="igpu-man-live-") as temporary:
        root = Path(temporary)
        environment = dict(os.environ, XDG_STATE_HOME=str(root / "state"), XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"))
        daemon = subprocess.Popen([str(BIN / "igpu-mand")], env=environment, stderr=subprocess.DEVNULL)
        try:
            for _ in range(100):
                result = subprocess.run([str(BIN / "igpu-man"), "status", "--json"], env=environment,
                                        capture_output=True, text=True, timeout=5)
                if result.returncode == 0:
                    require_collector(daemon)
                    snapshot = json.loads(result.stdout)
                    break
                time.sleep(.1)
            else:
                raise AssertionError("Collector did not start")
            assert snapshot["diagnostics"]["kwin_support"], "KWin support information unavailable"
            assert any(o["current_mode"] for g in snapshot["gpus"] for o in g["outputs"] if o["connected"])
            environment["IGPU_MAN_SCREENSHOT_TO"] = str(Path(sys.argv[1]).resolve())
            desktop = subprocess.run([str(BIN / "igpu-man-desktop")], env=environment,
                                     capture_output=True, text=True, timeout=20)
            assert desktop.returncode == 0, desktop.stderr
            assert "tray registered" in desktop.stderr, desktop.stderr
            print("PASS: live KWin diagnostics, current monitor modes, Plasma tray registration, OpenGL dashboard capture")
        finally:
            daemon.terminate(); daemon.wait(timeout=10)


if __name__ == "__main__":
    main()
