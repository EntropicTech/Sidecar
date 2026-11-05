#!/usr/bin/env python3
"""Capture the real dashboard in the current display, on a private session bus.

cargo build -p igpu-man-desktop --features screenshot
dbus-run-session -- python3 tests/desktop_smoke.py /tmp/igpu-man-dashboard.png
"""
from fixture_bus import require_empty, require_collector
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
    require_empty()
    screenshot = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory(prefix="igpu-man-desktop-") as temporary:
        root = Path(temporary)
        environment = dict(os.environ, XDG_STATE_HOME=str(root / "state"), XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"))
        daemon = subprocess.Popen([str(BIN / "igpu-mand")], env=environment, stderr=subprocess.DEVNULL)
        try:
            for _ in range(100):
                result = subprocess.run([str(BIN / "igpu-man"), "status", "--json"], env=environment,
                                        text=True, capture_output=True, timeout=5)
                if result.returncode == 0:
                    require_collector(daemon)
                    snapshot = json.loads(result.stdout)
                    print("Discovered GPUs:", [(g["alias"], g["pci"]) for g in snapshot["gpus"]])
                    break
                time.sleep(.1)
            else:
                raise AssertionError("Collector did not start")
            def cli(*args):
                return subprocess.check_output([str(BIN / "igpu-man"), *args], env=environment, text=True)
            if snapshot["gpus"]:
                cli("profile", "save", "browser-side", "--gpu", snapshot["gpus"][-1]["pci"], "--firefox", "--", "firefox")
            for label in ["UI-fixture-idle", "UI-fixture-video"]:
                record_id = json.loads(cli("record", "start", label))
                cli("record", "stop")
                cli("record", "import", str(record_id), str(ROOT / "tests/fixtures/mangohud.csv"))
            tabs = range(3) if "--all" in sys.argv else [int(environment.get("IGPU_MAN_SCREENSHOT_TAB", "0"))]
            for tab in tabs:
                output = screenshot.with_stem(screenshot.stem + f"-{tab}") if "--all" in sys.argv else screenshot
                environment["IGPU_MAN_SCREENSHOT_TO"] = str(output)
                environment["IGPU_MAN_SCREENSHOT_TAB"] = str(tab)
                subprocess.run([str(BIN / "igpu-man-desktop")], env=environment, check=True, timeout=20)
                assert output.is_file(), "Build the desktop with --features screenshot"
                print("Saved", output)
        finally:
            daemon.terminate(); daemon.wait(timeout=10)


if __name__ == "__main__":
    main()
