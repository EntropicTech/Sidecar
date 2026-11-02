# Troubleshooting and AI debugging

All diagnostic collection is local. No command uploads data. Share only the output
needed for the problem; recordings/exports may contain explicit profile arguments,
hardware identifiers and notes. Never share the entire config or process environment.

## Start here

```sh
igpu-man doctor
igpu-man status --json
igpu-man-desktop --status
igpu-man logs --component desktop --lines 100
igpu-man logs --component daemon --lines 100
```

Doctor returns JSON: hardware/capabilities, coverage, versions, current dashboard
state, storage/config paths, safe config counts/settings and the log directory.
Logs returns a JSON array of recent structured events. Increase `--lines` up to
1000. `--component` accepts `desktop`, `daemon`, `cli`, or `all`. A log reader skips
partial lines while another process is writing; it is not a forensic full-file
export. Inspect the private files directly if a complete local history is needed.
Doctor performs fresh discovery and reports collector reachability separately.
Logs and direct sampling work without the collector; direct mode is not the
collector's recording/status state.

Installed collector output also appears in:

```sh
journalctl --user -u igpu-man.service -n 100 --no-pager
```

## Window close or reopen trouble

Closing the window parks Sidecar in the system tray. Click its icon, choose
Open dashboard, or run `igpu-man-desktop` to reopen the same process. Choose
**Quit Sidecar** or use `igpu-man-desktop --quit` to exit dashboard/tray. The
collector, recordings and workloads continue independently. Without a tray,
Close exits; if the tray watcher disappears while parked, the window reopens.

```sh
igpu-man-desktop --status
igpu-man-desktop --close  # park when a tray is available
igpu-man-desktop         # reopen
igpu-man-desktop --quit  # exit dashboard and tray
```

A parked instance reports `visible: false`, `tray_resident: true`, unchanged
`updates`, and zero dashboard telemetry timestamp. No chart renderer or dashboard
telemetry/catalog polling runs while parked; collector sampling continues.
Look for `dashboard.parked`, `dashboard.window_reopened`, and
`dashboard.quit_requested`. Inspection/control helpers never start an instance.

Wayland cannot reliably hide a mapped window with the current toolkit. Sidecar
actually destroys its window and creates a fresh one using the cached event loop,
preserving its model and rebinding worker repaint callbacks. This replaces the
old cancel-close/hide failure. Quit and relaunch after a failed/poisoned worker;
closing/reopening alone retains that failed model and its recovery view.

## Collector unavailable, stale, or recording stopped

Start the collector in your desktop session:

```sh
systemctl --user start igpu-man.service
```

For an uninstalled build, run `./target/release/igpu-mand` in a terminal. Check
`daemon.failed`, `runtime.panic`, `recording.sample_failed` and
`recording.stop_persist_failed`. Inspect the storage limit, free disk space and
directory permissions. Do not delete the database to fix a startup failure; keep
it for investigation. An interrupted recording is recovered as interrupted, not
silently resumed.

A timeout may mean the operation is still completing. Check `status`/`record list`
before retrying Start/Stop/Keep. `record stop --expected-id ID` refuses to stop a
different recording. Expensive recording work is isolated from live sampling and
dashboard telemetry. A busy error means the bounded operation slots/queue were
full; wait for the current work before retrying. Large timeline/frame arrays can
hit a 64 MiB limit; export the recording instead of increasing memory implicitly.
`recording.writer_delayed` / `recording.writer_recovered` report persistence lag.
Samples are buffered in a bounded FIFO; exhaustion stops the collector visibly and
an unfinished recording recovers as interrupted. During Stop, an active ID with
a stopping diagnostic means earlier samples are draining, not that new samples
continue to be selected. Query-only read snapshots may lag a continuing recording.

## Correlate failures across components

Collector failures include a request ID in the CLI error. Find the same
`fields.request_id` in `api.request_completed` in the daemon log. Inspect its
operation, outcome and duration, then nearby recording/lifecycle events. Use
`boot_id` and `monotonic_ns` for ordering; wall clocks can change. A new PID or
`process.started` marks another process instance. `IGPU_MAN_LOG=debug` is opt-in;
do not enable arbitrary browser/media logging as a generic first step.

If file logging fails, the app falls back to JSON on stderr. Check the terminal or
user journal and fix ownership/space for the XDG state directory. Do not change
permissions to world-readable. Retention is bounded; collect relevant recent logs
before restarting dozens of times.

## Reproduction checklist for an AI or developer

Include the symptom and exact trigger, expected/actual result, whether the process
or just its window remained, installation method, desktop/session type, app version,
doctor output and relevant recent log events. Note any active recording or unkept
trial before requesting a restart. Exclude browsing data, URLs, secrets and full
environment dumps. The AI should read AGENTS.md, architecture, standards and the
review before editing, reproduce using isolated tests, then report concrete evidence.

Portable checks: `make check`, `make test`. Native lifecycle checks:

```sh
dbus-run-session -- python3 tests/dashboard_lifecycle.py --cycles 8
# Actual Plasma tray; refuses to touch a pre-existing dashboard:
python3 tests/dashboard_lifecycle.py --live --cycles 8
```

Private mode creates its own collector/recording and temporarily pauses **only that
test collector**. Never send SIGSTOP to the real collector as routine troubleshooting.

## Recovery, large recordings and mixed builds

All three binaries support `--help` and `--version` without a display or bus.
Compare source IDs in doctor, dashboard status and logs to detect mixed builds.
Doctor hashes its own running executable on demand; installed `build-info.json`
records release artifact hashes. Build identity adds no recurring sampling work.

The dashboard uses a GPU-only timeline and cached frame peaks. Full-frame
statistics and exports are unchanged. Point/frame limits return clear errors;
export complete data or use stopped-recording sample pages. Display buckets must
not be interpreted as counts of individual stutters.

Worker failure/poisoning opens a recovery view. Quit and relaunch for fresh workers;
the collector and launched apps continue. Inspect `dashboard.worker_failed`,
`dashboard.backend_poisoned`, `runtime.panic`, and status `worker_health`.

A Keep warning may mean the profile is already committed and usable. Startup
reconciles pending launcher intents without replacing conflicting files. Inspect
`profile.launcher_recovery_pending` and `profile.launcher_intent_cleanup_failed`;
preserve an existing launcher before deliberately resolving a conflict. Unknown
config keys are errors: correct the typo or use a compatible build. Do not delete
or rewrite the config to suppress the error.

For a manual collector upgrade, wait for the old process to exit, not merely for
its D-Bus name to disappear: SQLite ownership can outlive bus ownership briefly.
A coordinated systemd restart waits for process termination. Preserve live launch
registrations explicitly if coordinating a manual update; a collector restart by
itself does not persist trial evidence. Never interrupt a user's active recording
to update, or terminate their applications as upgrade cleanup.

## Generic Wayland/taskbar icon

The visible app is Sidecar; its technical identity is still `igpu-man`. Plasma
Wayland resolves the taskbar icon through `igpu-man.desktop` and its matching app
ID, not just the window's embedded PNG. Install the desktop entry and scalable
icon with the package or `make install`; a checkout alone supplies the custom tray
pixmaps but cannot register a launcher in Plasma. A local checkout launcher must
use the absolute path of `target/release/igpu-man-desktop` in Exec. Preserve any
existing launcher and use standard XDG application/icon locations. Refresh with
`kbuildsycoca6 --noincremental` after updating local launcher metadata if needed.

## Login autostart

Sidecar's XDG autostart entry must run `igpu-man-desktop --tray`; collector-only
service enablement does not add a tray icon. Enable the collector independently
with `systemctl --user enable igpu-man.service`. The packaged opt-in tray template
is `/usr/share/doc/igpu-man/igpu-man-autostart.desktop`. Copy it to your XDG config
`autostart/igpu-man.desktop`, or enable/disable it through Plasma Autostart. A source
checkout needs absolute release paths in Exec/TryExec and the user collector unit.

Verify with `igpu-man-desktop --status`: a genuine cold tray start reports
`windows_created: 0`, `updates: 0`, `visible: false`, `tray_resident: true`.
A duplicate `--tray` will not hide/reopen an existing window. If the tray is late,
Sidecar waits without graphics for up to 15 seconds; without a host it logs
`dashboard.tray_start_fallback` and opens a window. Normal application launch
opens the dashboard; Quit remains an exit rather than a request to restart.
No logout/reboot is required to simulate the startup command for validation.

## Removing a checkout dependency

For a per-user install use PREFIX=`$HOME/.local` and
USER_UNIT_DIR=`$HOME/.local/share/systemd/user`. The installed unit, launcher and
autostart template contain absolute prefix paths. Check `systemctl --user show
igpu-man.service -p FragmentPath -p ExecStart` for an old config override, and
inspect the Sidecar desktop/autostart entries for checkout paths. Remove only
confirmed old Sidecar overrides after backing them up, re-enable the installed
unit, and replace the autostart entry from the installed template. Preserve live
registrations when restarting and leave active recordings untouched. Verify
`/proc/PID/exe` resolves to the installed binaries, not the build directory.
Installed README includes per-user uninstall commands that work without source;
recordings/configuration/logs are retained.

## Failed developer update

Use `make update-user` for the user install; package-manager installs remain
pacman's responsibility. The tool refuses active recordings and visible editor
windows. Close/save first; do not force an update around the guard. Logs/journals
and two successful private backup sets live under the XDG state updates directory.
An incomplete journal blocks another update. Review it and run
`make recover-user RUN=/absolute/transaction/path`. Recovery restores programs,
not an old user database over newer data. A stopped service can be recovered;
active recording prevents recovery. A five-minute lease expires if no updater
survives. A temporary runtime unit override may remain after an interrupted update;
recovery removes it. See [updates/releases](updates-and-releases.md).


## Launch-path explanations and cached diagnostics

In Applications, expand **Launch path / next launch**, or inspect
`igpu-man candidates --json` → `launch_path`. A profile existing for an executable
does not establish how the running process started. **Unregistered** can follow a
manual collector restart; **unknown** can mean inaccessible executable identity.
Use a chosen profile for the next fresh launch, and inspect actual engine evidence
separately. Firefox's already-running check still refuses a fresh test without
terminating the existing browser. Managed elsewhere is respected.

Version information can be cached for five minutes; changed executable or Arch
package-database metadata triggers probes on the next 30-second diagnostic pass.
Display/KWin information retains its 30-second refresh. In dashboard `--status`,
`telemetry_wakeups` stays constant in a quiet tray; it advances on reopening or
explicit events. A status probe alone must not wake telemetry. Higher RAM after
opening a dashboard can reflect retained Rust/model/allocator state.
