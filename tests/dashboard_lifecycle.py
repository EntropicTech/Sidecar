#!/usr/bin/env python3
"""Exercise real native dashboard processes and their public lifecycle API.

dbus-run-session -- python3 tests/dashboard_lifecycle.py --cycles 8
python3 tests/dashboard_lifecycle.py --live --cycles 8
Live mode uses the Plasma tray but never changes the real collector's state.
Refuses to touch a pre-existing dashboard. Private mode also tests a paused
collector and an active recording using only its own temporary daemon/data.
"""
from fixture_bus import require_empty, require_collector
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BIN = Path(os.environ.get("IGPU_MAN_BIN_DIR", ROOT / "target/debug"))
GUI = BIN / "igpu-man-desktop"
BUS = "io.github.igpu_man.Dashboard"
REPORT = []


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--live", action="store_true")
    parser.add_argument("--cycles", type=int, default=8)
    parser.add_argument("--report", type=Path)
    parser.add_argument("--runtime-navigation", action="store_true", help="Validate that release builds ignore retired-view fixture settings")
    args = parser.parse_args()
    if not args.live:
        require_empty()
    with tempfile.TemporaryDirectory(prefix="igpu-man-lifecycle-") as temporary:
        root = Path(temporary)
        environment = {k: v for k, v in os.environ.items() if not k.startswith("IGPU_MAN_SCREENSHOT_")}
        environment.update(XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"), XDG_STATE_HOME=str(root / "state"))
        children = []
        stderr_paths={}
        daemon = None
        paused = False

        def gui(*options, check=True):
            return subprocess.run([str(GUI), *options], env=environment, text=True, capture_output=True, timeout=5, check=check)

        def status():
            return json.loads(gui("--status").stdout)

        def cli(*options):
            return json.loads(subprocess.check_output([str(BIN / "igpu-man"), *options], env=environment, text=True, stderr=subprocess.DEVNULL, timeout=8))

        def spawn():
            path = root / f"dashboard-{len(children)}.stderr"
            with path.open("w") as log:
                child = subprocess.Popen([str(GUI)], env=environment, stdout=log, stderr=log)
            children.append(child)
            stderr_paths[child.pid]=path
            return child

        def ready(child):
            for _ in range(100):
                if child.poll() is not None:
                    raise AssertionError(f"Dashboard {child.pid} exited during startup ({child.returncode}); stderr:\n{stderr_paths[child.pid].read_text()[-8000:]}")
                state = status()
                if state.get("ready") and state.get("pid") == child.pid:
                    return state
                time.sleep(.05)
            raise AssertionError("Dashboard did not become ready")

        def finish(child, method, state):
            started = time.monotonic()
            if method == "tray" and state.get("tray_available"):
                service = f"org.kde.StatusNotifierItem-{child.pid}-1"
                raw = subprocess.check_output(["busctl", "--user", "--json=short", "call", service,
                    "/MenuBar", "com.canonical.dbusmenu", "GetLayout", "iias", "0", "1", "0"], env=environment, text=True, timeout=3)
                entries = json.loads(raw)["data"][1][2]
                item = next(e["data"][0] for e in entries if e["data"][1].get("label", {}).get("data") == "Quit Sidecar")
                subprocess.run(["busctl", "--user", "call", service, "/MenuBar", "com.canonical.dbusmenu", "Event", "isvu", str(item), "clicked", "i", "0", "0"], env=environment, check=True, capture_output=True, timeout=3)
                actual = "Plasma tray Quit"
            else:
                gui("--close" if method == "close" else "--quit")
                actual = "Close" if method == "close" else "Quit"
                if method == "close" and state.get("tray_available"):
                    for _ in range(100):
                        if status().get("tray_resident"):break
                        time.sleep(.03)
                    else:raise AssertionError("Close did not park")
                    gui("--quit")
                    actual = "Close to tray + Quit"
            assert child.wait(timeout=3) == 0
            elapsed = time.monotonic() - started
            assert status().get("running") is False
            if state.get("tray_available"):
                watcher = subprocess.check_output(["qdbus6", "org.kde.StatusNotifierWatcher", "/StatusNotifierWatcher", "org.kde.StatusNotifierWatcher.RegisteredStatusNotifierItems"], env=environment, text=True, timeout=3)
                assert f"org.kde.StatusNotifierItem-{child.pid}-1/" not in watcher
            row = {"test": actual, "pid": child.pid, "seconds": round(elapsed, 4), "exit_code": child.returncode}
            REPORT.append(row)
            print(json.dumps(row), flush=True)

        assert status().get("running") is False, "A dashboard is already running; leave it untouched"
        live_before = None
        try:
            if args.live:
                live_before = cli("status", "--json")
            else:
                daemon = subprocess.Popen([str(BIN / "igpu-mand")], env=environment, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                for _ in range(100):
                    try:
                        cli("status", "--json")
                        require_collector(daemon)
                        break
                    except subprocess.CalledProcessError:
                        time.sleep(.05)
                else:
                    raise AssertionError("Temporary collector did not start")
                recording = cli("record", "start", "lifecycle-recording")
            if args.runtime_navigation:
                environment.update(IGPU_MAN_SCREENSHOT_TAB="4", IGPU_MAN_SCREENSHOT_WORKFLOW="catalog")
                child = spawn(); state = ready(child)
                for _ in range(100):
                    state = status()
                    if state.get("updates", 0) >= 2: break
                    time.sleep(.05)
                assert state.get("view") == 1, "A retired view escaped into runtime navigation; use release binaries for this check"
                finish(child, "quit", state)
                environment.pop("IGPU_MAN_SCREENSHOT_TAB"); environment.pop("IGPU_MAN_SCREENSHOT_WORKFLOW")
                print("PASS: runtime ignores retired-view fixture settings and opens Applications", flush=True)
            for cycle in range(args.cycles):
                child = spawn()
                state = ready(child)
                for _ in range(3):
                    assert gui().returncode == 0
                    assert status()["pid"] == child.pid, "Relaunch created another dashboard"
                finish(child, "close" if cycle % 2 == 0 else "tray", state)
                if daemon is not None:
                    assert cli("status", "--json")["active_recording"] == recording
            # Reopening can overlap the old instance's final frames/name release.
            for _ in range(2):
                old=spawn();ready(old)
                started=time.monotonic();gui("--quit")
                new=spawn()
                assert old.wait(timeout=3)==0
                REPORT.append({"test":"rapid reopen: old Quit","pid":old.pid,"seconds":round(time.monotonic()-started,4),"exit_code":old.returncode})
                state=ready(new)
                finish(new,"close",state)
            # Simultaneous cold starts must select one owner, not duplicate windows.
            burst = [spawn() for _ in range(5)]
            owner = None
            for _ in range(100):
                state = status()
                if state.get("ready"):
                    owner = next(p for p in burst if p.pid == state["pid"])
                    break
                time.sleep(.05)
            assert owner is not None
            for child in burst:
                if child is not owner:
                    assert child.wait(timeout=5) == 0
            finish(owner, "quit", state)
            if daemon is not None:
                child = spawn(); state = ready(child)
                daemon.send_signal(signal.SIGSTOP); paused = True
                time.sleep(1.2)  # Ensure the telemetry worker is waiting on an RPC.
                finish(child, "quit", state)
                assert REPORT[-1]["seconds"] < 1.0, "GUI quit waited for the collector RPC"
                daemon.send_signal(signal.SIGCONT); paused = False
                assert cli("status", "--json")["active_recording"] == recording
                cli("record", "stop")
                daemon.terminate(); daemon.wait(timeout=10); daemon = None
                child = spawn(); state = ready(child)
                finish(child, "close", state)  # Missing collector must not prevent closing.
            if live_before is not None:
                assert any(row["test"]=="Plasma tray Quit" for row in REPORT),"No actual tray Quit was exercised; inspect tray registration"
                after = cli("status", "--json")
                assert after["active_recording"] == live_before["active_recording"]
            for path in (root / "state/igpu-man/logs").glob("*.jsonl*"):
                for line in path.read_text().splitlines():
                    record = json.loads(line)
                    assert record["schema_version"] == 1 and "event" in record
            print(f"PASS: {len(REPORT)} clean lifecycle cycles; relaunch reuse, simultaneous starts, tray cleanup, structured logs" + ("; paused/missing collector and recording preservation" if not args.live else "; actual Plasma tray"))
        finally:
            for child in children:
                if child.poll() is None:
                    child.terminate()
                    child.wait(timeout=5)
            if daemon is not None and daemon.poll() is None:
                if paused:
                    daemon.send_signal(signal.SIGCONT)
                daemon.terminate(); daemon.wait(timeout=10)
            if args.report:
                args.report.write_text(json.dumps({"live_plasma": args.live, "cycles": REPORT}, indent=2) + "\n")


if __name__ == "__main__":
    main()
