#!/usr/bin/env python3
"""Real session-D-Bus integration in an isolated bus and XDG directory.

Run after cargo build: dbus-run-session -- python3 tests/integration.py
Never starts/stops/enables the installed user service or changes GPU settings.
"""
from fixture_bus import require_empty, require_collector
import json
import os
import re
from pathlib import Path
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BIN = Path(os.environ.get("IGPU_MAN_BIN_DIR", ROOT / "target/debug"))


def main():
    require_empty()
    with tempfile.TemporaryDirectory(prefix="igpu-man-integration-") as temporary:
        root = Path(temporary)
        environment = dict(os.environ, XDG_STATE_HOME=str(root / "state"), XDG_CONFIG_HOME=str(root / "config"),
                           XDG_DATA_HOME=str(root / "data"), XDG_DATA_DIRS=str(root / "system"),
                           QT_QPA_PLATFORM="offscreen")
        config = root / "config/igpu-man/config.toml"
        config.parent.mkdir(parents=True)
        config.write_text('schema_version=1\n[[profiles]]\nname="shortcut"\nexecutable="/usr/bin/true"\ngpu="0000:00:02.0"\n')
        daemon = None

        def cli(*args, success=True):
            result = subprocess.run([str(BIN / "igpu-man"), *args], env=environment,
                                    text=True, capture_output=True, timeout=20)
            if success:
                assert result.returncode == 0, (args, result.stderr)
            else:
                assert result.returncode != 0, args
            return result

        def raw_request(operation,payload):
            return subprocess.run(["dbus-send","--session","--print-reply","--dest=io.github.igpu_man.Collector","/io/github/igpu_man/Collector","io.github.igpu_man.Collector1.Request",f"string:{operation}",f"string:{payload}"],env=environment,text=True,capture_output=True,timeout=5)

        def start():
            process = subprocess.Popen([str(BIN / "igpu-mand")], env=environment,
                                       stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
            for _ in range(100):
                result = subprocess.run([str(BIN / "igpu-man"), "status", "--json"],
                                        env=environment, text=True, capture_output=True, timeout=5)
                if result.returncode == 0:
                    require_collector(process)
                    return process
                if process.poll() is not None:
                    raise AssertionError(process.stderr.read())
                time.sleep(.1)
            process.terminate()
            raise AssertionError("Collector did not start")

        try:
            daemon = start()
            snapshot = json.loads(cli("status", "--json").stdout)
            assert snapshot["api_version"] == 1
            # Launcher publication works without a GPU and never follows or
            # replaces an existing destination, even on an explicit retry.
            saved = json.loads(cli("profile", "list").stdout)
            shortcut = root / "data/applications/igpu-man-shortcut.desktop"
            for invalid in (
                {"name":"shortcut", "cli_path":"relative"},
                {"name":"shortcut", "cli_path":str(BIN / "igpu-man"), "unexpected":True},
                {"name":"shortcut"},
            ):
                assert raw_request("create_launcher", json.dumps(invalid)).returncode != 0
                assert not shortcut.exists()
            cli("profile", "launcher", "shortcut")
            content = shortcut.read_bytes()
            cli("profile", "launcher", "shortcut", success=False)
            assert shortcut.read_bytes() == content
            shortcut.unlink()
            target = root / "user-owned"
            target.write_text("preserve this file")
            shortcut.symlink_to(target)
            cli("profile", "launcher", "shortcut", success=False)
            assert shortcut.is_symlink() and target.read_text() == "preserve this file"
            assert json.loads(cli("profile", "list").stdout) == saved
            shortcut.unlink()
            cli("profile", "launcher", "missing", success=False)
            assert not shortcut.exists()
            cli("profile", "delete", "shortcut")
            # Management rules are executable identities and survive daemon restarts.
            cli("manage", "/tmp/workflow-owned-emulator", "--elsewhere")
            assert "managed_elsewhere" in (root / "config/igpu-man/config.toml").read_text()
            candidates = json.loads(cli("candidates", "--json").stdout)
            assert all("observed_seconds" in c and "management" in c and "launch_path" in c for c in candidates)
            schedule = json.loads(cli("experiment").stdout)
            assert len(schedule) == 9
            first = json.loads(cli("record", "start", "browser-idle").stdout)
            duplicate = subprocess.run([str(BIN / "igpu-mand")], env=environment,
                                       text=True, capture_output=True, timeout=5)
            assert duplicate.returncode != 0
            assert "already in use" in duplicate.stderr
            cli("record", "start", "duplicate", success=False)
            wrong=cli("record", "stop", "--expected-id", str(first+100), success=False)
            assert json.loads(cli("status","--json").stdout)["active_recording"]==first
            assert raw_request("stop",'{"expected_id":"invalid"}').returncode!=0
            assert json.loads(cli("status","--json").stdout)["active_recording"]==first
            request_id=re.search(r"request (\d+-\d+-\d+)",wrong.stderr).group(1)
            events=json.loads(cli("logs","--component","daemon","--lines","100").stdout)
            assert any(e["event"]=="api.request_completed" and e["fields"].get("request_id")==request_id and e["fields"]["outcome"]=="error" for e in events)
            time.sleep(.6)
            cli("mark", "integration marker")
            cli("record", "stop", "--expected-id", str(first))
            cli("record", "notes", str(first), "Playback smooth; manually observed")
            cli("record", "import", str(first), str(ROOT / "tests/fixtures/mangohud.csv"))
            detail = json.loads(cli("record", "show", str(first)).stdout)
            assert detail["recording"]["samples"] > 0
            assert detail["recording"]["markers"][0]["label"] == "integration marker"
            assert detail["frame_logs"][0]["aligned"] is False
            assert detail["frame_logs"][0]["summary"]["p99_ms"] == 40
            export = root / "export"
            cli("record", "export", str(first), str(export))
            assert (export / "frames-0.csv").read_text() == (ROOT / "tests/fixtures/mangohud.csv").read_text()
            cli("record", "export", str(first), str(export), success=False)
            bad = root / "bad.csv"; bad.write_text("frametime,elapsed\nNaN,1\n")
            cli("record", "import", str(first), str(bad), success=False)
            bad.write_text('"frametime,elapsed"\n10\n')
            cli("record", "import", str(first), str(bad), success=False)
            cli("status", "--json")  # Malformed headers must not poison the server mutex.
            config_path=root/"config/igpu-man/config.toml"
            previous_config=config_path.read_text()
            config_path.write_text('secret="DO_NOT_LOG_THIS_SENTINEL"\nbroken=[\n')
            invalid_config=cli("doctor",success=False)
            assert "DO_NOT_LOG_THIS_SENTINEL" not in invalid_config.stderr
            invalid_daemon=subprocess.run([str(BIN/"igpu-mand")],env=environment,text=True,capture_output=True,timeout=5)
            assert invalid_daemon.returncode!=0 and "DO_NOT_LOG_THIS_SENTINEL" not in invalid_daemon.stderr
            config_path.write_text(previous_config)
            logs=json.loads(cli("logs","--component","cli","--lines","1000").stdout)
            assert any(e["event"]=="cli.failed" and "TOML parse error (source omitted)" in json.dumps(e) for e in logs)
            assert "DO_NOT_LOG_THIS_SENTINEL" not in json.dumps(logs)
            rejected=raw_request("save_profile",json.dumps({"name":"bad","executable":"/usr/bin/true","gpu":"igpu","firefox":"DO_NOT_LOG_RPC_SENTINEL","_request_id":"123-456-789"}))
            assert rejected.returncode!=0 and "DO_NOT_LOG_RPC_SENTINEL" not in rejected.stderr
            assert raw_request("snapshot","{").returncode!=0
            assert raw_request("snapshot","[]").returncode!=0
            events=json.loads(cli("logs","--component","daemon","--lines","1000").stdout)
            assert "DO_NOT_LOG_RPC_SENTINEL" not in json.dumps(events)
            assert any(e["event"]=="api.request_rejected" for e in events)
            blocked=root/"blocked-state";blocked.write_text("not a directory")
            fallback=subprocess.run([str(BIN/"igpu-man"),"status","--json"],env=dict(environment,XDG_STATE_HOME=str(blocked)),text=True,capture_output=True,timeout=5)
            assert fallback.returncode==0 and '"event":"process.started"' in fallback.stderr
            if snapshot["gpus"]:
                pci = snapshot["gpus"][0]["pci"]
                applications = root / "data/applications"; applications.mkdir(parents=True, exist_ok=True)
                helper = root / "fixture-report"
                helper.write_text("#!/usr/bin/python3\nimport json,os,sys\nprint(json.dumps({'args':sys.argv[1:],'value':os.environ['IGPU_MAN_TEST_VALUE'],'prime':os.environ['DRI_PRIME'],'cwd':os.getcwd()}))\n")
                helper.chmod(0o700)
                original = applications / "fixture.desktop"
                original.write_text(f'[Desktop Entry]\nType=Application\nName=Fixture App\nExec=env IGPU_MAN_TEST_VALUE="two words" DRI_PRIME=wrong {helper} --prefix %U --suffix\nPath={root}\nIcon=utilities-terminal\nMimeType=text/plain;\nCategories=Utility;\n')
                before = original.read_text()
                entries = json.loads(cli("launchers", "--json").stdout)
                assert entries[0]["profile"]["desktop"]["file_argument"] == [1, "U"]
                cli("profile", "import", "imported", "fixture.desktop", "--gpu", pci)
                cli("profile", "import", "imported", "fixture.desktop", "--gpu", pci, success=False)
                urls = ["file:///tmp/a b", "https://example.invalid/?a=$(echo)&b=%U"]
                report = json.loads(cli("launch", "imported", "--", *urls).stdout)
                assert report["args"] == ["--prefix", *urls, "--suffix"]
                assert report["value"] == "two words" and report["cwd"] == str(root)
                assert report["prime"] == "pci-" + pci.replace(":", "_").replace(".", "_")
                cli("profile", "launcher", "imported")
                generated = applications / "igpu-man-imported.desktop"
                subprocess.run(["desktop-file-validate", str(generated)], check=True)
                assert "-- %U" in generated.read_text() and original.read_text() == before
                cli("profile", "delete", "imported")
                cli("profile", "save", "integration", "--gpu", pci, "--", "/usr/bin/printf", "%s", "exact argument")
                profiles = json.loads(cli("profile", "list").stdout)
                assert profiles[0]["gpu"] == pci
                result = cli("launch", "integration"); assert result.stdout == "exact argument"
                result = cli("run", "--gpu", pci, "--", "/usr/bin/printenv", "DRI_PRIME")
                assert result.stdout.strip() == "pci-" + pci.replace(":", "_").replace(".", "_")
                cli("profile", "launcher", "integration")
                launcher = root / "data/applications/igpu-man-integration.desktop"
                assert launcher.exists()
                cli("profile", "delete", "integration")
            second = json.loads(cli("record", "start", "video-igpu").stdout)
            daemon.kill(); daemon.wait(timeout=5)
            cli("status", success=False)
            daemon = start()
            assert "/tmp/workflow-owned-emulator" in (root / "config/igpu-man/config.toml").read_text()
            cli("manage", "/tmp/workflow-owned-emulator", "--restore")
            assert "/tmp/workflow-owned-emulator" not in (root / "config/igpu-man/config.toml").read_text()
            detail = json.loads(cli("record", "show", str(second)).stdout)
            assert detail["recording"]["state"] == "interrupted"
            comparison = json.loads(cli("record", "compare", str(first), str(second)).stdout)
            assert len(comparison["recordings"]) == 2
            marker = json.loads(cli("mark", "buffered capture").stdout)
            assert marker["capture_until_ns"] is not None
            cli("record", "stop")
            cli("record", "delete", str(first))
            cli("record", "show", str(first), success=False)
            print("PASS: D-Bus, guarded recording cleanup, correlated private logs, parser redaction, malformed CSV survival, recordings, ring capture, exports, profiles, launcher argv/URLs/environment/cwd, management persistence, crash recovery, reconnection")
        finally:
            if daemon and daemon.poll() is None:
                daemon.terminate(); daemon.wait(timeout=10)


if __name__ == "__main__":
    main()
