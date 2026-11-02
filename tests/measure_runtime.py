#!/usr/bin/env python3
"""Measure daemon, recording, and open-dashboard CPU on an isolated session bus.

Build release binaries first, then:
dbus-run-session -- python3 tests/measure_runtime.py /tmp/igpu-man-runtime.json
Opens the dashboard briefly; no games, browser restarts, or system tuning.
"""
import argparse
import json
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BIN = Path(os.environ.get("IGPU_MAN_BIN_DIR", ROOT / "target/release"))
HZ = os.sysconf("SC_CLK_TCK")


def usage(pid):
    fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
    cpu = int(fields[11]) + int(fields[12])
    children = int(fields[13]) + int(fields[14])
    memory = Path(f"/proc/{pid}/status").read_text()
    rss = next((int(line.split()[1]) for line in memory.splitlines() if line.startswith("VmRSS:")), 0)
    return cpu, cpu + children, rss


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument("--workflow", action="store_true")
    modes.add_argument("--collector-only", action="store_true")
    parser.add_argument("--seconds", type=float, default=30)
    parser.add_argument("--warmup", type=float, default=0)
    args = parser.parse_args()
    if not 10 <= args.seconds <= 300 or not 0 <= args.warmup <= 60:
        parser.error("Use 10–300 measurement seconds and 0–60 warm-up seconds")
    workflow = args.workflow
    results = {"seconds_per_mode": 30, "limitations": "Three sequential idle-desktop modes in one collector instance, one run each; later RSS includes earlier ring-buffer/recording allocations. Not a gaming comparison. CPU percentages use one core. Child CPU includes only reaped children. Private bus has no tray watcher; dashboard tests fallback window mode.", "modes": []}
    results["seconds_per_mode"] = args.seconds
    results["warmup_seconds"] = args.warmup
    if args.collector_only:
        results["limitations"] = "One collector-only observation on the current desktop workload; no recording/window. CPU uses one core and includes only reaped helper children. Tick quantization depends on duration; not a gaming result. Concurrent observers and changing process/GPU workload can affect readings."
    results["artifacts_sha256"]={name:hashlib.sha256((BIN/name).read_bytes()).hexdigest() for name in ["igpu-mand","igpu-man-desktop","igpu-man"]}
    if workflow:
        results["limitations"] = "Two sequential observations, collector-only then Applications view with live candidate queries. One run per mode on the current desktop workload; not a gaming comparison. CPU percentages use one core. Private bus has no tray watcher; temporary XDG settings, no managed trials. Later RSS includes the earlier ring buffer. CPU tick quantization depends on the selected duration."
    with tempfile.TemporaryDirectory(prefix="igpu-man-measure-") as temporary:
        root = Path(temporary)
        environment = dict(os.environ, XDG_STATE_HOME=str(root / "state"), XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"))
        daemon = subprocess.Popen([str(BIN / "igpu-mand")], env=environment, stderr=subprocess.DEVNULL)
        desktop = None

        def cli(*args):
            return subprocess.check_output([str(BIN / "igpu-man"), *args], env=environment, text=True, stderr=subprocess.DEVNULL)

        def measure(mode, processes):
            before = {name: usage(process.pid) for name, process in processes.items()}
            started = time.monotonic()
            time.sleep(args.seconds)
            elapsed = time.monotonic() - started
            output = {"mode": mode, "seconds": elapsed, "processes": {}}
            for name, process in processes.items():
                after = usage(process.pid)
                output["processes"][name] = {
                    "cpu_percent_one_core": (after[0] - before[name][0]) / HZ / elapsed * 100,
                    "cpu_with_reaped_children_percent": (after[1] - before[name][1]) / HZ / elapsed * 100,
                    "rss_mib": after[2] / 1024,
                }
            output["database_bytes"] = (root / "data/igpu-man/recordings.sqlite").stat().st_size
            files=list((root/"state/igpu-man/logs").glob("*.jsonl*"))
            output["diagnostic_log_bytes"]=sum(path.stat().st_size for path in files)
            output["diagnostic_log_lines"]=sum(sum(1 for _ in path.open()) for path in files)
            results["modes"].append(output)
            print(json.dumps(output), flush=True)

        try:
            for _ in range(100):
                try:
                    snapshot = json.loads(cli("status", "--json"))
                    break
                except subprocess.CalledProcessError:
                    time.sleep(.1)
            else:
                raise AssertionError("Collector unavailable")
            results["gpus"] = [{"pci": g["pci"], "name": g["name"]} for g in snapshot["gpus"]]
            results["coverage"] = snapshot["coverage"]
            time.sleep(args.warmup)
            measure("collector-only", {"collector": daemon})
            if args.collector_only:
                args.output.write_text(json.dumps(results, indent=2) + "\n")
                return
            if not workflow:
                cli("record", "start", "overhead-measurement")
                measure("recording-250ms", {"collector": daemon})
                cli("record", "stop")
            desktop = subprocess.Popen([str(BIN / "igpu-man-desktop"), *( ["--applications"] if workflow else [])], env=environment, stderr=subprocess.DEVNULL)
            time.sleep(2)
            measure("applications-open" if workflow else "dashboard-open", {"collector": daemon, "desktop": desktop})
        finally:
            if desktop and desktop.poll() is None:
                desktop.terminate(); desktop.wait(timeout=10)
            daemon.terminate(); daemon.wait(timeout=10)
    args.output.write_text(json.dumps(results, indent=2) + "\n")


if __name__ == "__main__":
    main()
