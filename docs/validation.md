# Validation

Automated and hardware results are recorded here during implementation. Hardware
checks require the actual desktop session; a unit test does not establish gaming
performance or suspend/resume correctness.

## Results — November 1, 2026

- Release binaries build successfully; formatting and Clippy checks pass.
- Fifty Rust tests cover accounting, availability, counter regression, cached
  sampling, process identities, launch arguments, parsing, storage limits,
  exclusive database ownership, crash recovery, and safe exports.
- `make check` passes formatting, all-target/all-feature Clippy and the offline
  documentation contracts. `make test` passes Rust tests and private-bus integration.
  CI is configured for these checks plus staged packaging and Xvfb/software-OpenGL
  lifecycle checks; the hosted CI job itself has not been run from this workspace.
- The private D-Bus integration suite passes profile persistence, actual child
  environment selection, recording/marker/import/export behavior, duplicate
  collector rejection, daemon crash recovery, and reconnection.
- All four native views rendered and were captured. Recording-view smoke data
  is explicitly synthetic and is not a performance result.
- A separate test against the live Plasma session passed KWin diagnostics,
  current monitor modes, Plasma tray registration, and an OpenGL window capture.
  No service was installed or enabled; all test data used temporary directories.
- Staged installation/removal, desktop-entry validation, package-script syntax,
  and systemd unit verification passed. Unit verification substituted the staged
  executable path because the system-wide binary is intentionally not installed.
- Discovery identified Navi 31 at `0000:03:00.0` and Granite Ridge at
  `0000:0e:00.0`, with the 4K/160 Hz main display and 1440p/~60 Hz side display.

### Application placement workflow

- Candidate fixtures cover sustained work versus spikes, unavailable/stale metrics,
  process reuse, sampling gaps, and persistent management exclusions.
- Launcher fixtures cover XDG overrides, quoted argv, escapes, field codes,
  environment assignments, duplicate names, and config/launcher rollback.
- The expanded private-bus integration test verifies imported file/URL arguments,
  working directory, GPU-selection precedence, desktop-file validation, and
  exclusion persistence through a collector restart.
- `tests/placement_workflow.py` passed on the Granite Ridge iGPU with a temporary
  native Wayland EGL demo and an isolated bus/configuration: imported launcher →
  fresh launch → sustained rendering verification → Keep plus generated launcher.
  Early Keep, duplicate Keep, existing-instance trials and post-exit verification
  were rejected. Decoder verification stayed unverified for this graphics workload.
  Managed elsewhere and Undo were checked against the running fixture app.
- Native layout captures cover Applications and the installed-launcher catalog,
  setup and live verification stages. These are UI/placement tests, not gaming or
  power-efficiency results. Existing browsers and development workflows were left
  running.

Reproduce after building the workspace:

```sh
dbus-run-session -- python3 tests/integration.py
dbus-run-session -- python3 tests/placement_workflow.py
# Optional UI captures; build with --features igpu-man-desktop/screenshot first:
IGPU_MAN_CAPTURE_WORKFLOW_DIR=/tmp/igpu-man-workflow \
  dbus-run-session -- python3 tests/placement_workflow.py
```

The updated release measured **0.63% of one CPU core** for the collector alone,
and **0.80% collector / 1.83% dashboard** with Applications open and live candidate
queries. These are two sequential 30-second observations on the current desktop
workload, one run each, not a gaming test or a repeatability guarantee. The latter
collector figure is 1.03% including reaped diagnostic subprocesses. CPU tick
quantization is about 0.033 percentage points. The older baseline below used a
different process population and dashboard view, so it is not a controlled A/B.
Raw results: [application workflow](measurements/application-workflow.json).
Reproduce with `dbus-run-session -- python3 tests/measure_runtime.py
/tmp/igpu-man-workflow-overhead.json --workflow`.

### Interface contrast pass (before auto-scale)

- Retained a dark palette with consistent orange iGPU / blue dGPU identities,
  colored primary actions, placement badges, and explicit warning/error labels.
- Enlarged the GPU charts and current readings, added labeled grid lines,
  brighter traces, a gradient area fill and current-sample dots. GPU utilization
  remains on a fixed 0–100% scale. Recording timelines use the same GPU colors
  with amber frametimes and purple event markers.
- All four native views plus the launcher/setup/live-verification stages were
  captured and inspected. A delayed overview capture exercises multiple live
  samples and the area-fill path. The isolated placement workflow passed again.
- Formatting, all-target/all-feature Clippy checks, and the release desktop build
  pass. The overhead observations above predate this visual pass; dashboard
  rendering overhead has not been remeasured for the new styling.

### Optional live-chart auto-scale

- Overview exposes Fixed 0–100% and Auto-scale, with fixed scale as the fresh-launch
  default. Auto-scale fits each GPU independently to all visible two-minute peaks,
  uses zero as the lower bound, and labels its current upper bound explicitly.
  Empty/idle histories use a 1% upper bound. Raw readings and recordings are unchanged.
- Three desktop tests cover low usage, retained peaks, fixed mode, ignored
  old/future/nonfinite samples, range bounds, and distinct fractional axis labels.
- Traces were reduced from 2.5 to 1.35 points, the glow was narrowed and softened,
  and live endpoint dots were reduced. Recording timelines retain fixed scales
  while sharing the thinner trace styling.
- Fixed and auto live views were rendered with actual samples and inspected;
  formatting, all-feature Clippy, the new tests, and the release desktop build pass.

### Lifecycle and quality hardening

- The old Wayland close path canceled closing and requested an unsupported hide,
  then stopped rendering. Close now exits the dashboard and tray; the collector,
  recording and launched applications remain independent.
- A rapid-reopen regression reproduced a lost Show: the old owner accepted it
  after Quit was queued but before the next UI frame. Shutdown intent is now
  immediate/monotonic, and reopening handles an owner disappearing during handoff.
  A Rust transition test and native overlapping-exit checks cover this failure.
- Final native checks passed **15 private-bus and 13 actual Plasma exits**, with
  zero exit codes, singleton reuse/concurrent starts, four actual tray Quit actions,
  tray/control-name cleanup, and responsive exit with a paused/missing collector.
  Private-mode recording remained active across all GUI cycles. Close/quit timings,
  including controller/status overhead, were below 0.07 seconds in this run.
  These exercise public Close/Quit controls and the actual tray; title-bar clicks
  were not separately automated. Raw [lifecycle evidence](measurements/dashboard-lifecycle.json).
- Integration covers request-ID correlation, private logs and stderr fallback,
  source redaction before IPC/terminal logging, malformed JSON/CSV without poisoning
  the server, guarded Stop with wrong/malformed IDs, and interrupted recordings.
  Rust fixtures cover atomic/private saves, symlink safety, bounded reads, log
  rotation/retention, and clearing a failed recording's previous timeline.
- The native placement test verifies pending settings survive dashboard exits,
  Keep protects saved settings, and Discard leaves the launched process running.
  The [recovery UI](images/trial-recovery.png) was captured and inspected.

After hardening, one sequential 30-second release observation per mode produced:

| Mode | Collector CPU | Collector + reaped child CPU | Dashboard CPU | Collector / dashboard RSS |
| --- | ---: | ---: | ---: | ---: |
| Collector, 1 Hz | 0.47% | 0.47% | — | 23.4 MiB / — |
| Recording, 4 Hz | 1.53% | 1.77% | — | 75.4 MiB / — |
| Dashboard open, 1 Hz | 0.73% | 1.03% | 0.23% | 89.4 / 125.0 MiB |

CPU uses one core as 100%. Current desktop processes, a separate existing collector,
and the real Wayland session were present. Window occlusion/compositor throttling
were not controlled; dashboard CPU is not a worst-case rendering bound. Later RSS
includes earlier ring/recording allocations. CPU quantization is about 0.033
percentage points. This is not a controlled A/B, repeatability study or gaming
result. Collector-only usage met the <1% target in this observation. Total diagnostic
logs grew from 2,054 bytes/7 events after the first mode to 4,499 bytes/16 events
after all three modes, including explicit commands and GUI lifecycle events.
Raw [hardening overhead](measurements/quality-hardening.json).

Reproduce with `dbus-run-session -- python3 tests/measure_runtime.py /tmp/igpu-man-quality.json`.
For staged packaging, build release binaries then run `python3 tests/packaging.py`.
The staged install/removal and archive checks passed with every documentation
asset included. Desktop entry, staged user-service unit and PKGBUILD syntax checks
passed; verification does not install, enable or start the real user service.

### Earlier idle-desktop overhead

The following baseline predates the application-placement workflow. It is retained
for comparison and is not a measurement of the added candidate/verification queries.

One 30-second observation per mode using release binaries, a private D-Bus
session, and the real desktop's processes. Modes ran sequentially in one
collector instance; later RSS includes the earlier recording/ring buffer.
CPU values use **one CPU core as 100%**.

| Mode | Collector CPU | Collector + reaped child CPU | Desktop CPU | Collector / desktop RSS |
| --- | ---: | ---: | ---: | ---: |
| Collector, 1 Hz | 0.57% | 0.57% | — | 22.0 MiB / — |
| Recording, 4 Hz | 1.63% | 1.93% | — | 71.4 MiB / — |
| Dashboard open, 1 Hz | 0.73% | 1.00% | 1.10% | 84.7 / 122.9 MiB |

The initial implementation scanned every descriptor on every tick and measured
1.43% collector CPU / 5.53% recording CPU with the same procedure. Caching
descriptor discovery for five seconds reduced those readings; known clients
are still sampled every tick. Before/after accessible process counts were
140/131, both with 17 unique DRM clients and four permission failures.

Raw results: [before](measurements/before-descriptor-cache.json) and
[after](measurements/after-descriptor-cache.json). Reproduce with
`dbus-run-session -- python3 tests/measure_runtime.py /tmp/igpu-man-runtime.json`.
This test briefly opens a dashboard window.

These are single-run idle observations, not a game benchmark or a guarantee of
sub-1% overhead on every workload. There is no measured run-to-run range yet;
CPU tick quantization is approximately 0.033 percentage points over 30 seconds.
The initial 0.067% CLI result obtained inside the restricted sandbox had limited
process visibility and is not used as the full-desktop result. Frame effects,
GPU-rendering overhead, actual MangoHud game capture, and Firefox decoding on
both GPUs still need the controlled hardware experiments below.

## Required hardware acceptance

- Check both PCI identities/render nodes and monitor connections against
  `lspci`, `drm_info`, and `kscreen-doctor -j`.
- Compare GPU and per-process readings with `amdgpu_top` over idle, graphics,
  and video workloads. Expect different sampling windows; do not require exact
  percentage equality.
- Fully restart Firefox with each GPU profile. Confirm WebRender and video
  behavior through `about:support`, decoder activity, CPU readings, and playback.
- Unplug/reconnect the side monitor and suspend/resume. After at most one
  diagnostics refresh (30 seconds), confirm identities, modes, and activity.
- Restart the collector while the dashboard is open; verify the stale banner
  and recovery. Verify dashboard Close, reopening, marking, and Quit.
- Run the nine-run, three-condition gaming comparison, then repeat after an update.
- Compare monitoring off, tray/dashboard closed, dashboard open, and recording.
  Use at least three runs per mode. Target collector-only CPU below 1% of one
  core at 1 Hz, and game-frame differences below the measured repeatability
  limit. Report CPU, memory, and frame effects rather than assuming low overhead.


## Recording isolation and remaining review — November 1, 2026

`make check`, `make test`, release build/manifest and staged packaging passed.
There are 48 Rust tests. Additional native checks passed: 15 private-bus lifecycle
cycles, installed-launcher/iGPU placement workflow, and recording-worker failure,
telemetry-worker failure, backend poison and fresh reopen. The
[worker recovery view](images/worker-recovery.png) was captured and inspected.
Native parser tests exercise help/version/unknown/conflicting arguments with no
display or bus and verify no config/database is created. Keep fault-point fixtures
cover committed intent, published artifact, cleanup, conflict and symlink states.
Strict configuration compatibility, cursor duplicate timestamps and peak retention
have fixtures. Hosted CI is configured, not claimed executed here.

Under approximately 128 MiB of synthetic recording reads, the old release's
maximum observed status-call latency was 867 ms. The final release completed
18 read requests while 25 status calls remained below 6.7 ms; nine distinct live
samples and ten persisted samples were observed. Stop during a concurrent export
took 6.3 ms. A separate debug/native run kept the dashboard telemetry advancing
while reading, with a maximum status latency of 20 ms and Stop at 10.5 ms.
These are finite stress observations including CLI startup, not hard latency
bounds. The committed regression uses generous 750 ms failure thresholds.

Final release footprint, one sequential 30-second observation per mode:

| Mode | Collector CPU | Collector + reaped child CPU | Dashboard CPU | Collector / dashboard RSS |
| --- | ---: | ---: | ---: | ---: |
| Normal 1 Hz | 0.67% | 0.70% | — | 23.1 MiB / — |
| Recording 4 Hz | 1.80% | 2.07% | — | 78.1 MiB / — |
| Dashboard open, 1 Hz | 0.87% | 1.10% | 0.20% | 95.3 / 125.0 MiB |

CPU uses one core. The first ring is not filled; later RSS includes prior high-rate
recording allocations. Window occlusion/throttling is uncontrolled. The real
process population had 21 observed DRM clients and four permission failures; a
separate existing collector remained running. Normal collector CPU met the <1%
target, including reaped children, in this observation. Dashboard CPU is not a
worst-case rendering bound. Logs contained seven events/2,976 bytes after normal
collection and sixteen events/6,416 bytes after all modes, including explicit
commands and GUI lifecycle. No per-sample logging was introduced.

Two alternating observations per old/initial-isolation build measured normal
collector CPU at 0.60% before and 0.60–0.63% after; recording at 1.83–1.87% before
and 1.67–1.73% after. They cover the first isolation patch, not every subsequent
hardening change. Do not infer a causal dashboard-CPU improvement from uncontrolled
occlusion. CPU quantization is approximately 0.033 percentage points. Full
[before/after evidence and artifact identities](measurements/recording-isolation.json)
preserve the separate runs and limitations.

Reproduce after `make build`:

```sh
IGPU_MAN_BIN_DIR=target/release dbus-run-session -- python3 tests/recording_responsiveness.py --report /tmp/responsiveness.json
dbus-run-session -- python3 tests/measure_runtime.py /tmp/overhead.json
# Optional screenshot-feature debug build first for this native recovery test:
dbus-run-session -- python3 tests/worker_recovery.py /tmp/worker-recovery
```

Limits remain explicit: filesystem calls cannot be forcibly canceled, WAL/free
pages can exceed the live-page budget, oversized timeline projections fail with
export/paging guidance, and heavy requested I/O still consumes CPU/disk bandwidth.
No game-frame effect, long-run memory plateau, monitor reconnect or suspend/resume
claim follows from these tests. Those hardware acceptance tasks remain above.

The running manual collector/dashboard were updated after testing. An initial
updater race (bus release preceding database-lock release) triggered rollback;
waiting for process exit allowed a clean update. Final source identities matched,
both dashboard workers were healthy, both live registrations and the single
saved profile were preserved, original recording IDs 1/2 remained, and only owned
upgrade checkpoints were deleted. Firefox and other workloads were not restarted.

## Sidecar identity — November 1, 2026

The display name is Sidecar; technical commands, data paths, service/bus names and
launcher filename remain `igpu-man`. The selected paired blue/orange SVG generates
checked-in 24/32/48 px tray PNGs and a 128 px window PNG. No dependency, polling
task or animation was added; decoding happens once at successful dashboard startup.

`make check`, `make test` (48 Rust tests plus isolated integration/responsiveness),
release/manifest and staged packaging passed. Eleven private native lifecycle
cycles passed with clean exits, singleton reuse and recording preservation. The
[native Sidecar window](images/sidecar-dashboard.png) was captured and inspected.
The live Plasma tray exposed the Sidecar title and three correctly sized ARGB
pixmaps; alpha and both exact GPU colors were verified through D-Bus. The local
launcher validated and `kiconfinder6` resolved its scalable icon. A panel-pixel
capture was not automated. The running utility was updated with healthy workers;
two live registrations, one profile and original recording IDs 1/2 were retained.
Firefox and other workloads were not restarted. Prior performance measurements
remain historical; this visual pass did not remeasure GPU/frame-time overhead.

## Close to tray — November 1, 2026

This supersedes the historical exit-on-close lifecycle behavior above. Native
Close destroys the Wayland window/renderer and parks Sidecar's Rust model and tray.
Tray Activate or Show recreates the window using the cached on-demand event loop,
with the same PID/view/drafts and rebound repaint callbacks. Only Quit seals
process shutdown. A native close echo was observed overwriting rapid Show intent;
a guard against repeated close intent plus native regression coverage fixes it.

`make check`, `make test` (50 Rust tests), release/manifest and staged packaging
passed. A private native SNI fixture and actual Plasma each passed six close/tray
Activate cycles with the same process/view, stopped parked redraw counts, sixteen
rapid Close/Show attempts and actual tray Quit while parked. Private watcher
loss reopened the window; watcher return restored the tray. Eleven additional
no-tray lifecycle cycles passed, including paused/missing collector behavior and
recording preservation. Worker failure/poison recovery captures also passed.
The actual title-bar button was not separately automated: public Close and native
window teardown/recreation were exercised on Wayland. Hosted CI is configured,
not claimed executed here.

One sequential 30-second release observation per mode on actual Plasma measured
0.47% of one core / 344 UI updates with the window open, then no measured CPU
ticks / zero UI updates while parked. CPU quantization is approximately 0.033
percentage points; this is not a universal zero-overhead guarantee. RSS was
127.7 MiB open and 127.1 MiB parked: resident state/allocator or driver caches remain,
so this does not claim that all RAM is returned. Dashboard telemetry/catalog
polling stops while parked; the independent collector keeps sampling. Window
occlusion/workload were uncontrolled, and no GPU or game-frame effect was measured.
Raw [tray lifecycle/overhead evidence](measurements/tray-residency.json).

Reproduce native residency with:

```sh
cargo build --workspace --locked
cargo build -p igpu-man-desktop --example tray_fixture --locked
dbus-run-session -- python3 tests/tray_lifecycle.py
# Refuses to touch an existing dashboard; keeps the real collector untouched:
python3 tests/tray_lifecycle.py --live
```

## Tray-only login startup — November 1, 2026

`--tray` now initializes the model, sleeping workers and SNI service before any
native window/event loop/GL context. `windows_created` and UI `updates` both remain
zero until Show/Activate. Private startup tests passed without a display, with a
late host, and with a permanently missing host (15-second fallback). Duplicate
startup preserved both parked and open states. Cold Quit originally reproduced a
NoReply race; bounded graceful D-Bus reply draining resolved it. Ordinary tray
lifecycle, watcher reconnect and eleven no-tray/paused-collector cycles passed.

`make check`, `make test` (50 Rust tests plus integration/responsiveness), release
manifest and staged packaging passed, including the opt-in desktop template.
CI is configured to run native startup tests under Xvfb; hosted execution is not
claimed. No feature requires new runtime dependencies or recurring startup polling.

For the requested live setup, the validated absolute-path checkout autostart entry
was registered under the user's XDG autostart directory. The collector's matching
user unit was installed, enabled and started after a reversible manual-to-service
migration. Both live registrations, one saved profile and recording IDs 1/2 were
preserved; only the owned upgrade checkpoint was deleted. No user recording was
active and Firefox/other workloads were not restarted.

Executing the configured tray command on actual Plasma produced `tray_resident:
true`, `windows_created: 0`, `updates: 0`; a duplicate command reused the same PID.
The collector reported active/running/enabled. Initial tray process RSS was about
11 MiB, a single observation rather than a long-run bound. The checkout binaries
must remain at their configured absolute paths. Login behavior was simulated
without logging the user out or rebooting; those session transitions were not
performed.

## Independent per-user installation — November 1, 2026

A system-wide pacman install required an unavailable interactive administrator
password. Sidecar was instead installed for this account under `~/.local`, without
changing system-wide packages. All three release binaries are copies in
`~/.local/bin`, with icon, launcher, license, documentation and build manifest in
`~/.local/share`; the unit uses `~/.local/share/systemd/user/igpu-man.service`.

The installer now renders absolute executable paths from PREFIX/USER_UNIT_DIR.
System and per-user staged installation/removal passed, including a prefix with
spaces and desktop-file validation. `make check` passed. No Rust/backend behavior
changed in this installation pass; previous Rust test results remain above.

Known checkout-specific user files were backed up before migration. The old
higher-priority config unit was removed, the installed unit re-enabled, and tray
autostart replaced from the installed template. Both live registrations, one
profile and original recording IDs 1/2 survived the managed service restart; no
user recording was active. Installed collector/desktop executable paths and
working directories were verified outside the checkout, with active/enabled
service and tray-only zero-window startup. Browser/other workloads were not
restarted. The checkout was not renamed/deleted as a test.

The installed binaries subsequently passed the private cold-start suite (including
missing display, delayed/missing hosts, duplicate startup and Quit). Each installed
binary's SHA-256 matched the installed release manifest. A targeted audit found
no checkout references in active autostart, user units or application launchers.
`igpu-man` resolves through the user's PATH to `~/.local/bin/igpu-man`.


## Coordinated updates and public upstream — November 1, 2026

Source was reviewed for private data and published to
[EntropicTech/Sidecar](https://github.com/EntropicTech/Sidecar). Local `make check`,
`make test` (50 Rust tests plus native arguments, integration, maintenance lease
and responsiveness), release/manifest, staged system/user packaging and update
transaction fixtures passed. The maintenance tests use a real private-bus collector;
the updater fixtures fake systemd, not the collector. Success preserves byte-identical
configuration/recordings/autostart, active capture is refused, restart failure restores
programs, and an interrupted stopped service can recover. Malformed/wrong nonces,
restart-carried exclusion and continued sampling under a lease are exercised.
The bounded stdin RPC bridge avoids putting registration payloads in argv.

The first older live install required a deliberate bootstrap; subsequent actual
`make update-user` runs completed the normal staged/leased/restarted/verified path.
The installed service stayed enabled and returned active, one saved profile and
original recordings 1/2 survived, and both live launch identities were preserved.
No browser or other workload was restarted. Private data backups and the last two
successful program rollback sets are retained under the XDG state directory.
The latest native startup remained tray-only; no logout/reboot was performed.

A hosted non-root Arch-container build completed a real package and all 50 Rust
checks; it exposed a missing hicolor-icon-theme dependency and GCC/Rust mixed-LTO
link incompatibility. The recipe now declares the icon theme/direct libgcc, disables
GCC object LTO while retaining Cargo thin LTO, and CI fails on namcap errors rather
than trusting its exit status alone. Runtime-loaded-library warnings are explained
in the release guide. Hosted checks are tracked in
[Actions](https://github.com/EntropicTech/Sidecar/actions); a previous revision's
success is not exact-source release acceptance. No AUR submission, devtools
clean-chroot acceptance, CachyOS inclusion, suspend/resume result or new gaming/
collector-overhead measurement is claimed by this update pass.


The first full portable hosted run passed backend/update/rollback/staged packaging
and then exposed an X11-only missing `libxkbcommon-x11` runtime library at native
window creation. A local private-bus X11 startup passed normally and reproduced
the panic at xkbcommon-dl's X11 loader when only that library was withheld via a
test-child-only dlopen shim. The explicit Arch/Ubuntu dependency declarations now
include it; startup assertions include child status and bounded fixture-owned
structured logs so hosted failures retain the panic location. This did not require
changing production window lifecycle code. Final exact-source hosted results
remain visible in Actions rather than inferred from the local reproduction.


## Launch-path audit and background work — November 1, 2026

`make check` and `make test` passed (53 Rust tests plus native arguments,
integration, update lease and large-recording responsiveness). Fixtures cover
registered profiles/trials/commands, unregistered and inaccessible identities,
exclusions, multiple saved targets, actual registered intent taking precedence,
older candidate responses, and retained wrapper attribution versus PID reuse.
Version-cache tests count probes across unchanged identities, expiry, and missing/
installed/replaced tools. Private native startup tests assert stable telemetry
wake-up counters for cold and warm tray parking, then successful reopen. Six
tray cycles/rapid reopen/watcher recovery, eleven lifecycle cases, both worker
failures/poison and a captured/inspected Applications audit view passed. No user
browser or other workload was restarted; visual capture used a temporary profile.

With two readers over a synthetic 127.98 MiB recording, the native live check
observed 22 snapshot requests, no errors, maximum request latency 0.031 seconds,
Stop during export 0.019 seconds, and three distinct dashboard telemetry updates.
These include CLI overhead and do not establish real-time guarantees.

Short sequential 30-second before/after observations varied: normal collector plus
reaped helpers was 0.70% before and 0.83–0.93% after; recording 2.13% before and
2.20–2.27% after; open-dashboard collector 1.00% before and 0.77–1.10% after.
Dashboard CPU was 0.20% before and 0.20–0.23% after. Those uncontrolled samples
cannot establish an overall CPU win or attribute their variation to code changes.

A longer concurrent pair (separate private buses/XDG trees, five-second warm-up,
120 seconds each, old and new release binaries, no windows/recording) measured:

| Collector | Own CPU | CPU including reaped helpers | Resident RAM |
| --- | ---: | ---: | ---: |
| Before | 0.83% | 1.16% | 61.5 MiB |
| After | 0.83% | 0.90% | 61.3 MiB |

One core is 100%. The helper-inclusive difference was about 0.26 percentage points;
the collector's own CPU was unchanged at this resolution. Both observed 19 DRM
clients initially, with 207/217 inspected processes during staggered startup.
Two concurrent observers, changing desktop workloads and a single pair limit
interpretation; this is not a run-to-run distribution or game-frametime result.
The optimized normal collector met the sub-1% target in this observation.
Version probes avoid repeated subprocesses; GPU/process sampling rates and live
hardware/connector discovery remain unchanged. Cold/parked wake-up assertions
establish event-driven telemetry, not universal zero CPU or reclaimed-RAM claims.

Raw [background/audit measurements](measurements/launch-audit-background.json).
Reproduce modes with `tests/measure_runtime.py`; for a longer collector-only run:

```sh
dbus-run-session -- python3 tests/measure_runtime.py /tmp/background.json --collector-only --seconds 120 --warmup 5
# IGPU_MAN_BIN_DIR can select a preserved before build; independent private buses
# can run a pair concurrently. Do not use the real service as test cleanup.
```

Installation/update checks retain the existing coordinated updater safety gates.
Native suspend/resume, physical monitor reconnect and gaming benefit are not
claimed by this pass. Hosted exact-source outcomes are recorded in Actions.



Hosted portable checks and the Arch package build passed for implementation
revision 147873e. Standalone private-bus validation does not establish installed
systemd-user service accounting coverage. An additional real-unit acceptance check
is needed (Q22): filesystem mount sandboxing in a user manager can implicitly
create a user namespace and prevent cross-process procfs access. Any service-policy
change requires separate review; the current unit has not been changed by this pass.
No private live-install diagnostics are published in this service caveat.
