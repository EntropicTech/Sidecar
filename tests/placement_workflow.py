#!/usr/bin/env python3
"""Hardware smoke test on a private bus/XDG tree. Opens one temporary EGL window.

dbus-run-session -- python3 tests/placement_workflow.py
Does not restart browsers, modify installed launchers, or change GPU settings.
"""
from fixture_bus import require_empty, require_collector
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BIN = Path(os.environ.get("IGPU_MAN_BIN_DIR", ROOT / "target/debug"))
BUS = "io.github.igpu_man.Collector"
OBJECT = "/io/github/igpu_man/Collector"
INTERFACE = "io.github.igpu_man.Collector1"


def main():
    require_empty()
    with tempfile.TemporaryDirectory(prefix="igpu-man-placement-") as temporary:
        root = Path(temporary)
        environment = dict(os.environ, XDG_STATE_HOME=str(root / "state"), XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), XDG_DATA_DIRS=str(root / "system"))
        daemon = subprocess.Popen([str(BIN / "igpu-mand")], env=environment,
                                  stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        child = None

        def cli(*args, success=True, input=None):
            result = subprocess.run([str(BIN / "igpu-man"), *args], env=environment,
                                    input=input, text=True, capture_output=True, timeout=15)
            assert (result.returncode == 0) == success, (args, result.stderr)
            return result.stdout

        def rpc(method, payload, success=True):
            result = subprocess.run(["qdbus6", BUS, OBJECT, INTERFACE + ".Request", method,
                                     json.dumps(payload)], env=environment, text=True,
                                    capture_output=True, timeout=15)
            assert (result.returncode == 0) == success, (method, result.stderr)
            return json.loads(result.stdout) if success else None

        def capture(stage, trial=None):
            directory = os.environ.get("IGPU_MAN_CAPTURE_WORKFLOW_DIR")
            if not directory:
                return
            directory = Path(directory); directory.mkdir(parents=True, exist_ok=True)
            output = directory / f"{stage}.png"
            capture_env = dict(environment, IGPU_MAN_SCREENSHOT_WORKFLOW=stage,
                               IGPU_MAN_SCREENSHOT_WAIT_MS="1500",
                               IGPU_MAN_SCREENSHOT_TO=str(output), IGPU_MAN_SCREENSHOT_TRIAL=json.dumps(trial))
            subprocess.run([str(BIN / "igpu-man-desktop")], env=capture_env,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True, timeout=20)
            assert output.exists(), "Build with igpu-man-desktop/screenshot for UI captures"

        try:
            for _ in range(100):
                result = subprocess.run([str(BIN / "igpu-man"), "status", "--json"],
                                        env=environment, text=True, capture_output=True, timeout=5)
                if result.returncode == 0:
                    require_collector(daemon)
                    snapshot = json.loads(result.stdout)
                    break
                time.sleep(.1)
            else:
                raise AssertionError("Collector did not start")
            igpu = next((g for g in snapshot["gpus"] if g["integrated"]), None)
            if igpu is None or shutil.which("eglgears_wayland") is None:
                print("SKIP: iGPU and eglgears_wayland are required")
                return
            # A unique executable avoids interacting with any existing gear demos.
            fixture = root / "placement-fixture"
            shutil.copy2(shutil.which("eglgears_wayland"), fixture)
            applications = root / "data/applications"; applications.mkdir(parents=True)
            original = applications / "fixture.desktop"
            original.write_text(f"[Desktop Entry]\nType=Application\nName=Placement Fixture\nExec={fixture}\nIcon=video-display\nCategories=Utility;\n")
            original_text = original.read_text()
            entry = rpc("launchers", {})[0]
            profile = entry["profile"]
            profile["name"] = "trial-placement-fixture"
            profile["gpu"] = igpu["pci"]
            capture("catalog")
            capture("setup")
            child = subprocess.Popen([str(BIN / "igpu-man"), "try-profile"],
                                     env=environment, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
            child.stdin.write(json.dumps(profile).encode()); child.stdin.close()
            fields = Path(f"/proc/{child.pid}/stat").read_text().rsplit(")", 1)[1].split()
            process = {"pid": child.pid, "start_ticks": int(fields[19])}
            keep = {"process": process, "name": "fixture-kept", "create_launcher": True,
                    "cli_path": str(BIN / "igpu-man")}
            rpc("keep_trial", keep, success=False)  # No evidence yet.
            assert json.loads(cli("profile", "list")) == []
            verified = None
            for _ in range(80):
                if child.poll() is not None:
                    raise AssertionError(child.stderr.read().decode())
                verified = rpc("verify_trial", {"process": process})
                if verified["can_keep"]:
                    break
                time.sleep(.25)
            else:
                raise AssertionError(verified)
            assert verified["rendering"] == "target_gpu", verified
            assert verified["decoding"] != "target_gpu", verified
            capture("verify", {"profile": profile, "process": process})
            pending=rpc("pending_trials",{})
            assert len(pending)==1 and pending[0]["process"]==process
            # Dashboard instances used for the captures exited; the collector still
            # owns the exact launch settings, without saving an unverified profile.
            assert pending[0]["profile"]["executable"]==str(fixture)
            capture("recover")
            result = rpc("keep_trial", keep)
            assert rpc("pending_trials",{})==[]
            rpc("discard_trial",{"process":process},success=False)  # Saved settings are protected.
            generated = Path(result["launcher_path"])
            assert generated.exists() and original.read_text() == original_text
            subprocess.run(["desktop-file-validate", str(generated)], check=True)
            saved = json.loads(cli("profile", "list"))[0]
            assert saved["verification"]["rendering"] is True
            assert saved["verification"]["decoding"] is False
            cli("try-profile", input=json.dumps(profile), success=False)  # Existing instance.
            rpc("keep_trial", keep, success=False)  # Never overwrite the saved profile.
            # Managed elsewhere moves the live app out of the ranked candidates.
            cli("manage", str(fixture), "--elsewhere")
            candidates = json.loads(cli("candidates", "--json"))
            candidate = next(c for c in candidates if c["executable"] == str(fixture))
            assert candidate["management"] == "managed_elsewhere"
            cli("manage", str(fixture), "--restore")
            candidates = json.loads(cli("candidates", "--json"))
            assert next(c for c in candidates if c["executable"] == str(fixture))["management"] == "saved_profile"
            child.terminate(); child.wait(timeout=10); child = None
            time.sleep(1.2)
            assert not rpc("verify_trial", {"process": process})["can_keep"]
            # Route the same original desktop ID, then observe a fresh ordinary
            # launch. Only this owned EGL fixture is stopped/reopened.
            setup={"profile":"fixture-kept","desktop_id":"fixture.desktop","cli_path":str(BIN/"igpu-man"),"login":False,"default_browser":False}
            preview=rpc("normal_launch_preview",setup)
            rpc("normal_launch_apply",{"setup":setup,"token":preview["token"]})
            child=subprocess.Popen([str(BIN/"igpu-man"),"launch","--normal","fixture-kept"],env=environment,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)
            fields=Path(f"/proc/{child.pid}/stat").read_text().rsplit(")",1)[1].split()
            managed_process={"pid":child.pid,"start_ticks":int(fields[19])}
            for _ in range(80):
                assert child.poll() is None
                verified=rpc("verify_trial",{"process":managed_process})
                if verified["rendering"]=="target_gpu":break
                time.sleep(.25)
            else:raise AssertionError(verified)
            assert verified["decoding"]!="target_gpu"
            candidates=rpc("candidates",{})
            automatic=next(c for c in candidates if c["launch_path"]["registration"]==managed_process)
            assert automatic["verification"]["rendering"]=="target_gpu" and automatic["verification"]["decoding"]!="target_gpu"
            rpc("normal_launch_undo",{"profile":"fixture-kept"})
            assert original.read_text()==original_text and child.poll() is None
            # Reusing a saved name with different intent cannot inherit this
            # process's automatic confirmation after Undo.
            changed=dict(saved,args=["--different-launch-settings"])
            rpc("save_profile",changed)
            candidates=rpc("candidates",{})
            automatic=next(c for c in candidates if c["launch_path"]["registration"]==managed_process)
            assert automatic["verification"] is None
            rpc("save_profile",saved)
            child.terminate();child.wait(timeout=10);child=None
            child=subprocess.Popen([str(BIN/"igpu-man"),"try-profile"],env=environment,stdin=subprocess.PIPE,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE)
            child.stdin.write(json.dumps(profile).encode()); child.stdin.close()
            for _ in range(60):
                pending=rpc("pending_trials",{})
                if pending and pending[0]["process"]["pid"]==child.pid:break
                assert child.poll() is None
                time.sleep(.1)
            else:raise AssertionError("Trial was not recoverable")
            rpc("discard_trial",{"process":pending[0]["process"]})
            assert rpc("pending_trials",{})==[] and child.poll() is None
            assert len(json.loads(cli("profile","list")))==1
            print("PASS: imported launcher → fresh iGPU launch → sustained render verification → Keep + normal-launch routing → measured iGPU rendering → Undo; pending setup survives dashboard exits; discard leaves app alive; early Keep/existing instance/overwrite/exit protections; Managed elsewhere + Undo")
        finally:
            if child is not None and child.poll() is None:
                child.terminate(); child.wait(timeout=10)
            daemon.terminate(); daemon.wait(timeout=10)


if __name__ == "__main__":
    main()
