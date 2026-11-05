#!/usr/bin/env python3
"""Advanced saved-profile desktop routing on a private bus and temporary XDG roots.

Does not open/restart user apps, change real defaults, or operate real services.
GPU-dependent apply/exec checks skip clearly without an AMD iGPU; parser,
transaction and PID reuse regressions also run as portable Rust tests.
"""
from fixture_bus import require_empty, require_collector
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BIN = Path(os.environ.get("IGPU_MAN_BIN_DIR", ROOT / "target/debug")).resolve()


def main():
    require_empty()
    with tempfile.TemporaryDirectory(prefix="sidecar-normal-") as temporary:
        root = Path(temporary)
        env = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"),
                   XDG_DATA_HOME=str(root / "data"), XDG_STATE_HOME=str(root / "state"),
                   XDG_DATA_DIRS=str(root / "system"), XDG_CONFIG_DIRS=str(root / "system-config"))
        daemon = None
        manager = None
        profile_name = "fixture-igpu"
        desktop_id = "org.sidecar.Fixture.desktop"
        apps = root / "system/applications"
        apps.mkdir(parents=True)
        helper = root / "fixture program"
        shutil.copy2("/usr/bin/printf", helper)
        original = apps / desktop_id
        original_text = ("[Desktop Entry]\nType=Application\nName=Routing Fixture\n"
                         f'Exec="{helper}" "%%s|%%s|%%s" base %U tail\n'
                         "Icon=video-display\nMimeType=text/plain;\nDBusActivatable=true\n"
                         "Actions=other;\n[Desktop Action other]\nExec=/usr/bin/true\n")
        original.write_text(original_text)
        startup = root / "config/autostart/old-fixture.desktop"
        startup.parent.mkdir(parents=True)
        startup_text = f'[Desktop Entry]\nType=Application\nName=Old startup\nExec="{helper}" background\n'
        startup.write_text(startup_text)
        session = root / "config/ksmserverrc"
        session.write_text("[General]\nexcludeApps=existing-app\nloginMode=restorePreviousLogout\n[Other]\nkeep=true\n")
        mime = root / "config/mimeapps.list"
        mime.write_text("[Default Applications]\nx-scheme-handler/http=old.desktop;\nimage/png=viewer.desktop;\n")

        def cli(*args, success=True, payload=None):
            result = subprocess.run([str(BIN / "igpu-man"), *args], env=env, input=payload,
                                    text=True, capture_output=True, timeout=35)
            assert (result.returncode == 0) == success, (args, result.stderr)
            return result

        def rpc(method, data, success=True):
            return cli("internal-request", method, payload=json.dumps(data), success=success)

        def start():
            child = subprocess.Popen([str(BIN / "igpu-mand")], env=env,
                                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            for _ in range(100):
                result = subprocess.run([str(BIN / "igpu-man"), "status", "--json"], env=env,
                                        text=True, capture_output=True, timeout=5)
                if result.returncode == 0:
                    require_collector(child)
                    return child, json.loads(result.stdout)
                assert child.poll() is None, "Owned fixture collector exited"
                time.sleep(.1)
            child.terminate(); child.wait(timeout=5)
            raise AssertionError("Owned collector did not start")

        def capture(stage):
            directory = os.environ.get("IGPU_MAN_CAPTURE_NORMAL_DIR")
            if directory:
                output = Path(directory) / f"normal-{stage}.png"
                output.parent.mkdir(parents=True, exist_ok=True)
                capture_env = dict(env, IGPU_MAN_SCREENSHOT_TAB="2", IGPU_MAN_SCREENSHOT_TO=str(output),
                                   IGPU_MAN_SCREENSHOT_NORMAL=stage)
                subprocess.run([str(BIN / "igpu-man-desktop")], env=capture_env, check=True,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=25)
                assert output.exists(), "Build with screenshot feature"

        try:
            daemon, sample = start()
            assert json.loads(cli("profile", "check-normal").stdout) == []
            igpu = next((g for g in sample["gpus"] if g["integrated"]), None)
            if igpu is None:
                rpc("normal_launch_preview", {"profile":"missing", "desktop_id":"../escape.desktop",
                    "cli_path":str(BIN / "igpu-man"), "login":True, "default_browser":False}, success=False)
                print("SKIP: successful GPU-backed normal-launch API/exec checks need an AMD iGPU")
                return
            cli("profile", "import", profile_name, desktop_id, "--gpu", igpu["pci"])
            setup = dict(profile=profile_name, desktop_id=desktop_id, cli_path=str(BIN / "igpu-man"),
                         login=False, default_browser=False)
            preview = json.loads(rpc("normal_launch_preview", setup).stdout)
            assert not (root / "data/applications" / desktop_id).exists()
            assert startup.read_text() == startup_text
            capture("preview")
            original.write_text(original_text + "# changed after preview\n")
            rpc("normal_launch_apply", {"setup":setup,"token":preview["token"]}, success=False)
            assert not (root / "data/igpu-man/normal-launches.json").exists()
            original.write_text(original_text)
            preview = json.loads(rpc("normal_launch_preview", setup).stdout)
            active = json.loads(rpc("normal_launch_apply", {"setup":setup,"token":preview["token"]}).stdout)[0]
            assert active["launcher_ok"] and active["login_ok"] is None and active["restore_excluded"] is None
            managed = root / "data/applications" / desktop_id
            assert "DBusActivatable=false" in managed.read_text() and "Desktop Action" not in managed.read_text()
            assert startup.read_text() == startup_text
            assert original.read_text() == original_text
            journal = root / "data/igpu-man/normal-launches.json"
            assert journal.stat().st_mode & 0o777 == 0o600
            rpc("normal_launch_preview",dict(setup,login=True),success=False)
            assert session.read_text().startswith("[General]\nexcludeApps=existing-app\n")
            assert cli("launch", "--normal", profile_name, "--", "one argument").stdout == "base|one argument|tail"
            cli("profile", "delete", profile_name, success=False)
            cli("profile", "save", profile_name, "--gpu", igpu["pci"], "--", str(helper), success=False)
            # The same ordinary desktop ID works after collector restart.
            daemon.terminate(); daemon.wait(timeout=10)
            manager=subprocess.Popen([str(ROOT/"target/debug/examples/service_fixture"),str(BIN/"igpu-mand")],env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True)
            import select
            assert select.select([manager.stdout],[],[],5)[0], "Fixture user manager did not become ready"
            assert manager.stdout.readline().strip()=="ready"
            assert not (root/"config/autostart"/desktop_id).exists()
            command=next(line[5:] for line in managed.read_text().splitlines() if line.startswith("Exec="))
            arguments=shlex.split(command);arguments=arguments[:-2]  # no URL on this launch
            result=subprocess.run(arguments,env=env,text=True,capture_output=True,timeout=20)
            assert result.returncode == 0 and result.stdout == "base|tail|"
            assert json.loads(cli("profile", "check-normal").stdout)[0]["phase"] == "active"
            latencies = []
            for _ in range(8):
                before = time.monotonic(); cli("status", "--json"); latencies.append((time.monotonic()-before)*1000)
            capture("active")
            # A later outside edit is kept; a failed Undo must retain recovery data.
            managed_text = managed.read_text(); managed.write_text(managed_text + "# external edit\n")
            cli("profile", "undo-normal", profile_name, success=False)
            assert managed.read_text().endswith("# external edit\n") and journal.exists()
            managed.write_text(managed_text)
            # Unrelated additions survive successful Undo.
            session.write_text(session.read_text().replace("excludeApps=", "excludeApps=later-app,"))
            cli("profile", "undo-normal", profile_name)
            assert not managed.exists() and not (root/"config/autostart"/desktop_id).exists()
            assert startup.read_text() == startup_text and original.read_text() == original_text
            assert "existing-app" in session.read_text() and "later-app" in session.read_text()
            assert "fixture program" not in session.read_text() and "keep=true" in session.read_text()
            # Default handlers use the original desktop ID, with per-key undo.
            profile = json.loads(cli("profile", "list").stdout)[0]
            profile["firefox"] = True  # Synthetic adapter; never launches a real browser.
            rpc("save_profile", profile)
            setup["default_browser"] = True
            preview = json.loads(rpc("normal_launch_preview", setup).stdout)
            recording = json.loads(cli("record", "start", "guard").stdout)
            rpc("normal_launch_apply", {"setup":setup,"token":preview["token"]}, success=False)
            cli("record", "stop", "--expected-id", str(recording))
            rpc("normal_launch_apply", {"setup":setup,"token":preview["token"]})
            assert f"x-scheme-handler/http={desktop_id};" in mime.read_text()
            assert "image/png=viewer.desktop;" in mime.read_text()
            mime.write_text(mime.read_text() + "application/pdf=reader.desktop;\n")
            cli("profile", "undo-normal", profile_name)
            assert "x-scheme-handler/http=old.desktop;" in mime.read_text()
            assert "application/pdf=reader.desktop;" in mime.read_text()
            assert "x-scheme-handler/https=" not in mime.read_text()
            # Public inputs cannot inject recovery operations or arbitrary paths.
            malicious = dict(setup, changes=[{"path":"/tmp/never"}])
            rpc("normal_launch_preview", malicious, success=False)
            assert json.loads(cli("profile", "check-normal").stdout) == []
            no_url=apps/"no-url.desktop";no_url.write_text(f'[Desktop Entry]\nType=Application\nName=No URL\nExec="{helper}"\n')
            cli("profile","import","no-url","no-url.desktop","--gpu",igpu["pci"])
            no_url_profile=next(p for p in json.loads(cli("profile","list").stdout) if p["name"]=="no-url")
            no_url_profile["firefox"]=True;rpc("save_profile",no_url_profile)
            rpc("normal_launch_preview",dict(setup,profile="no-url",desktop_id="no-url.desktop"),success=False)
            print(f"PASS: preview freshness, ordinary argv/URL routing, on-demand user service after restart, no app-startup or restore changes, per-key defaults, original preservation, Undo conflicts, recording guard; max status latency {max(latencies):.1f} ms (8 local calls)")
        finally:
            if manager is not None and manager.poll() is None:
                manager.stdin.write("quit\n");manager.stdin.flush();manager.stdin.close();manager.wait(timeout=10)
            if daemon is not None and daemon.poll() is None:
                daemon.terminate(); daemon.wait(timeout=10)


if __name__ == "__main__":
    main()
