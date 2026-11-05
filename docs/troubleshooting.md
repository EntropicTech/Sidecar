# Troubleshooting and AI debugging

All diagnostic collection is local. No command uploads data. Share only the output
needed for the problem; recordings/exports may contain explicit profile arguments,
hardware identifiers and notes. Never share the entire config or process environment.

## Find the relevant guide

- [Set up normal iGPU launches](#set-up-normal-igpu-launches)
- [Black window after Close](#black-sidecar-window-after-title-bar-close)
- [Developer update recovery](#failed-developer-update)
- [Launcher conflicts and busy workers](#launcher-conflicts-and-dashboard-work-admission)
- [Install, login setup and removal](setup.md)

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



For a user-service collector with unavailable per-process accounting, compare its
procfs access with a standalone collector before attributing the failure to a GPU
driver. User-service mount sandboxing can implicitly create a user namespace even
when the direct PrivateUsers property is off. Review the effective unit and its
namespace requirements; standalone tests do not validate the systemd execution
context. The shipped compatibility policy is `PrivateTmp=no`, `ProtectSystem=no`,
`PrivateUsers=no`, retaining `NoNewPrivileges=yes` and `UMask=0077`. Confirm with
`systemctl --user show igpu-man.service -p PrivateTmp -p ProtectSystem -p PrivateUsers -p NoNewPrivileges`.
An old installed unit or user override can retain the namespace restrictions;
use the guarded updater when no recording is active. Compare installed coverage
and the collector's `/proc/PID/ns/user` with the caller after restart. Do not add
root/ptrace capabilities or change global kernel permissions. Read the explicit
sandbox tradeoff in [security](security.md) before changing this policy. See the public
[systemd execution documentation](https://freedesktop.org/software/systemd/man/latest/systemd.exec.html)
and [procfs access-mode checks](https://man.archlinux.org/man/ptrace.2.en).


Thread counts exceed the configured async worker count: the collector also has
its main thread, ordered writer and on-demand blocking workers; the desktop has
native/UI workers and a dependency-managed D-Bus executor. The tray's own async
runtime uses its existing dedicated thread. A quiet tray can therefore retain
threads and virtual address-space reservations while consuming no measurable CPU
in a short observation. Virtual memory is not resident RAM. For isolated local
measurements build release binaries and the tray_fixture example, then run
`dbus-run-session -- python3 tests/measure_threads.py /tmp/threads.json`.
The script never changes the installed service. Keep machine-specific reports
local unless sharing is explicitly authorized.


## Security-related input and resource errors

A busy `LimitsExceeded` reply means that request was not queued. Live reads have
four slots, recording reads two, and mutations eight; live/read blocking work retains its
slot if the caller disconnects. Back off ordinary reads and inspect
state before retrying a mutation. Envelope-size errors also use LimitsExceeded.

Trial wrappers require bounded stdin; an old dashboard sending profile JSON in
argv needs a coordinated `make update-user` to replace all three binaries.
A pending-launcher input rejection protects internal recovery state; do not
remove it to import a profile. Normal saved-profile launches omit that internal
field while preserving the durable recovery intent. Existing launcher conflicts
remain warnings. Diagnostic output over 64 KiB, non-UTF-8 or two seconds is
unavailable, not a partial successful result. See [security maintenance](security.md).


## Recording notes, comparisons and interrupted exports

Legacy widget fixtures use checkbox selection for notes/comparison and separate
timeline inspection; the installed dashboard has no recording page. Each recording retains its own
unsaved draft during selection changes and tray parking; Save notes writes only
that recording. Save drafts before Quit or a developer update.

A duplicate-ID comparison is rejected: choose different runs. Settings warnings
may describe intentional GPU-condition changes; inspect the run details/notes.
Missing launch context cannot be repaired by merely creating a matching saved
profile. Repeat the experiment through an explicit launch if verification is needed.
Warnings identify categories without exposing profile argument/environment values.

An INCOMPLETE marker means an export is in progress, failed or interrupted. Wait
for an active export or review the error, then retry to a new absolute directory.
Do not remove the marker to label partial data complete. Existing failed bundles
are preserved, and the saved recording is the source for re-export. For an older
bundle of uncertain origin, re-export from the recording. A timeout is uncertain;
the server may still finish. Synchronization cannot override filesystem guarantees.


## Black Sidecar window after title-bar Close

A previous native teardown defect could log dashboard.parked while KWin still
owned an undrawn Sidecar window. That window could become unresponsive even though
the tray/control channel answered. The reviewed eframe patch drains deferred
window retirement before the application parks. A SIGABRT/core with no Rust panic
can reflect an externally requested abort; inspect the signal origin and actual
window state rather than assuming a renderer panic or GPU reset.

After updating, native_window_active must become false and no native Sidecar
window should remain while parked. Closing through the CLI alone is not sufficient
to certify title-bar behavior. The explicit regression is
`python3 tests/wayland_window_lifecycle.py --live --cycles 6`; it refuses an existing
real dashboard, uses private fixture names/data and controls only its owned
windows via short-lived KWin scripts. It requires Plasma Wayland/qdbus6 and journal
access; no KWin settings or user service state are changed.

Keep core dumps and machine-specific diagnostics private. Diagnostic collection
should filter exact process IDs/components; do not publish complete user journals,
application memory or environment dumps. Third-party patch changes must pass
`python3 tests/verify_vendor.py`; see [provenance](../vendor/README.md).

## Set up normal iGPU launches

Use **Applications → Add app → Use iGPU**, then close/reopen the app normally when
ready. No profile name, test session, special shortcut or app login-startup setting
is required. The wrapper can start the installed collector user service on demand;
check that igpu-man.service is installed and not masked if launch fails. The
Sidecar window need not stay open. Existing single-instance browsers keep their
old settings until closed; unknown Firefox instances are refused with a fixed
local notification. An uncoordinated collector restart can lose registration.

The app card separates its saved shortcut from measured rendering/video activity.
Use the app; play a video for decoder evidence. Waiting, missing accounting and
stale collection are not success or proof of software decoding. **Details → Check shortcut**
checks file state; **Undo** restores originals and removes unchanged auto-created
settings. Existing profiles/recordings are preserved. A timeout is uncertain: check
state before retrying. Use Finish setup for interrupted preparation, or repeat Undo
for an undoing phase. Keep the private recovery journal; do not delete it to clear
an error. Conflicting external edits are preserved and require local review.

Direct commands, alternate desktop IDs, app-owned startup or some restored
sessions can bypass a launcher. An ID-based pin follows the override; if an old
pin points directly at the packaged file, re-pin from the application menu.
Desktop action submenus are omitted and restored by Undo. Underlying-launcher
warnings after package updates need Undo/review/reapply. Native launcher matches
are suggested; custom workflows and detailed inspection remain in Launch Settings or the CLI.

## Remove an earlier version's app login changes

Use **Remove previous login changes** on that app, or
`igpu-man profile remove-login PROFILE`. This keeps normal iGPU routing while
restoring the prior startup entries and removing only Sidecar-added exclusions.
It never disables unrelated startup settings or changes Sidecar's own tray startup.
A removing_login phase can be retried; a conflict retains all recovery data and
outside edits. New setup rejects login:true and CLI --login is removed.

Custom profiles are in **Launch Settings**. Recordings remain accessible through
the CLI with data intact; the installed dashboard does not poll their catalog.


## GPU activity names and hidden diagnostics

Usage, power, temperature and charts are the main readings. Expand **Device
details** for PCI/render paths, the reported raw device name, label source and
other sensors; **Collection & display details** contains coverage and KWin output.
Missing/permission/stale states remain distinct, with partial-coverage warnings
visible even when details are closed. Auto-scale retains a labeled axis per GPU.

Friendly names use kernel product metadata or an exact device/revision entry in
`/usr/share/libdrm/amdgpu.ids`. Missing iGPU names can use host CPU context (for
example Ryzen 7 9800X3D graphics); this is not a claim of an exact GPU model. Missing
optional metadata falls back to the diagnostic description. Database/CPU metadata
is read once per collector lifetime. If package updates change those names,
refresh through a safe collector restart/guarded developer update when no recording
is active. Display labels do not alter PCI routing, saved profiles or recordings.


## Dashboard navigation

The only tabs are **Applications**, **GPU Activity** and **Launch Settings**.
Applications is the default. **More → Edit profile** and **Open profile editor** in
Launch Settings reveal the form; rendering/recording options, Steam commands and
custom normal routing remain expandable. New setup never starts apps at login.

The Advanced menu and the placement/recording pages have been removed from the
installed dashboard. Existing recordings are preserved: use `igpu-man record list`,
`show`, `notes`, `import`, `compare` or `export`. Use `igpu-man record stop` if a
recording-enabled profile is capturing. CLI/API compatibility does not restore the
removed tabs. Prior screenshots/captures of retired views are validation artifacts.

For developer updater validation use `dbus-run-session -- python3 tests/update_user.py`.
The harness also self-isolates when invoked directly; existing service owners cause
an immediate refusal. A ready API is insufficient evidence of fixture ownership:
the helper checks its collector peer PID before mutations. Do not bypass those
checks to run a test against the installed service.


Applications now uses compact, full-width rows. Normal-launch intent is shown under
the app name; Rendering and Video decoding describe measured activity separately.
Hover those readings for waiting/unavailable/stale guidance. Details contains the
launcher identity and Check shortcut; Undo and recovery/conflict warnings remain
visible. Narrow windows wrap activity onto a second line rather than hiding it.


Launch Settings uses compact full-width profile rows. The target GPU remains
visible on wide/narrow layouts; full executable paths and normal-launch ownership
are in More. Edit/Delete stay disabled for a managed profile until its routing is
undone. The editor and saved settings are unchanged; expanding the form does not
launch or move an app. Launch buttons use the requested target GPU's color.


## Launcher conflicts and dashboard work admission

Creating a separate launcher in Launch Settings or `igpu-man profile launcher`
never replaces an existing file or symlink. A conflict preserves it; inspect the
existing shortcut locally before deciding to remove it or use another profile
name. Ordinary iGPU routing and profiles are unaffected. After a request timeout,
check for the shortcut before retrying: the writer may have completed publication.
Parent-directory sync warnings mean publication happened but crash durability is
uncertain, not that the shortcut was rolled back.

“Sidecar is busy; this action was not queued” means the bounded dashboard work
queue rejected that action before dispatch. Wait for current work, then retry
explicitly. `dashboard.action_rejected` records only the operation/reason.
`dashboard.worker_unavailable` means the worker stopped; restart the dashboard
and inspect logs. Close/Quit use a separate control path and remain available.

Integration fixtures refuse occupied Sidecar/user-manager session buses. Run them
with `dbus-run-session -- python3 tests/<fixture>.py`. Do not weaken the guard or
stop the installed collector to make a fixture run. Live read-only checks have an
explicit mode and do not use synthetic recording/config mutations.


## Source-free per-user removal

First undo managed routes while the CLI still exists, quit Sidecar, disable its
tray autostart entry and stop/disable `igpu-man.service` as described in
[setup](setup.md#uninstall). For the default manual user prefix, these are the
installed program assets; they exclude private config/data/state:

```sh
rm -f "$HOME/.local/bin/igpu-man" "$HOME/.local/bin/igpu-mand" "$HOME/.local/bin/igpu-man-desktop"
rm -f "$HOME/.local/share/systemd/user/igpu-man.service" "$HOME/.local/share/applications/igpu-man.desktop"
rm -f "$HOME/.local/share/icons/hicolor/scalable/apps/igpu-man.svg" "$HOME/.local/share/licenses/igpu-man/LICENSE"
rm -rf -- "$HOME/.local/share/doc/igpu-man"
systemctl --user daemon-reload
```

Use the matching prefix if you installed elsewhere. Do not use manual removal for
pacman-owned files. Saved profiles, recordings and recovery journals remain until
you explicitly delete them; keep the journal if any routing Undo is unresolved.
