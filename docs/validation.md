# Validation

Automated and hardware results are recorded here during implementation. Hardware
checks require the actual desktop session; a unit test does not establish gaming
performance or suspend/resume correctness.

## Current acceptance summary

The latest recorded local code-quality pass is
[November 4](#code-quality-hardening--november-4-2026): 80 Rust tests plus private-bus
integration, release lifecycle/tray/startup, staged packaging and updater fixtures
passed. The fresh advisory gate passed with five visible maintenance notices.
[Code review](code-review.md) tracks remaining work and acceptance criteria.

The current dashboard is [three tabs](#three-tab-dashboard--november-4-2026).
Older entries below document previous builds, including removed trial/recording
pages and retired app login-startup features; they are not the current user guide.
Use [Everyday use](user-guide.md) and [setup](setup.md) for current instructions.

Latest short local release observations measured collector CPU at 0.47% of one
core and about 16.2 MiB RSS over 15 seconds. Cold tray had zero measured CPU ticks
and no redraw/polling wake-ups, with about 10.6 MiB RSS. These are single-run
observations, not a universal resource bound or gaming-performance claim. Physical
reconnect, suspend/resume, Firefox playback and gaming repeatability remain
separate [hardware acceptance checks](#required-hardware-acceptance).

## Historical evidence

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
  and recovery. Verify dashboard Close, reopening and Quit, plus CLI markers.
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


## Async runtime sizing — November 1, 2026

The collector's async executor is configured with two workers; the SNI service
uses a current-thread runtime driven by its existing dedicated thread. Blocking
jobs, ordered writes and the dependency-owned D-Bus executor remain independent.
No global runtime environment or installed-service sandbox setting is changed.

Local `make check`/`make test` passed (53 Rust tests plus private-bus integration,
maintenance-lease and recording responsiveness). Native cold startup without a
display, delayed/missing tray host, quiet cold/warm parking, reopen/rapid activation,
watcher recovery, eleven dashboard lifecycle cases and worker failure/poison
recovery passed. Native synthetic large-recording reads preserved live snapshots,
Stop and dashboard telemetry progress. Staged packaging and isolated updater
success/refusal/rollback/recovery checks passed.

An isolated release benchmark supports before/after CPU, resident and virtual
memory, OS thread counts, cold-tray window/update/wake counters and binary hashes.
Measurement outputs are explicitly requested local files with private permissions;
machine-specific results are kept local, not included in the public repository.
Short observations and an uncontrolled desktop do not establish a universal CPU,
physical-memory, game-frametime or total OS-thread bound. Virtual address-space
reservation must not be described as resident RAM. The existing installed-service
procfs compatibility issue (Q22) is a separate acceptance item.

Reproduce a local observation after building the release and tray fixture:

```sh
cargo build -p igpu-man-desktop --example tray_fixture --locked
dbus-run-session -- python3 tests/measure_threads.py /tmp/threads.json --seconds 60
# IGPU_MAN_BIN_DIR can point at a preserved before build. Independent session
# buses can run a pair concurrently; never use the real service as test cleanup.
```


## Security boundaries — November 1, 2026

Q24–Q28 are covered by local formatting/all-feature Clippy, documentation contracts,
57 Rust tests and private-bus integration. New security fixtures reproduce wrapper
argv inspection using only owned processes and synthetic environment values, reject
public recovery intents at save/registration/trial boundaries, restart the fixture
collector without publishing a launcher, exercise malformed/oversized stdin and
preserve exact arguments and explicit environment values. They refuse an existing
collector and never restart a real browser or change GPU settings.

Admission tests fill each class, prove busy rejection precedes JSON parsing, recover
from parse failures, and retain live blocking permits after caller cancellation.
Existing interrupted-Keep/conflict/symlink tests still pass. Diagnostic tests cover
exact byte limits, excessive and continuous output, invalid UTF-8, nonzero exits,
timeout and direct-helper reaping. Output is never captured to disk.

Native iGPU trial verification/Keep, cold tray, delayed/missing hosts, park/reopen,
watcher recovery and eleven dashboard lifecycle cases passed. Synthetic large-read
checks preserved sampling/live RPC/Stop progress. Staged install/removal and private
updater success/refusal/rollback/recovery passed. Release CPU/RSS and quiet-tray
observations use the existing private measurement tool; machine-specific reports
remain local. Short uncontrolled observations do not establish gaming benefit or
a universal overhead bound.

The pinned cargo-audit 0.22.2 gate passed locally against current RustSec data,
reporting five unmaintained-package notices, no matched vulnerability advisories,
and no ignored advisories. Follow-ups and limits are in [security](security.md).
CI configuration adds scheduled checks and Dependabot; hosted results require the
changes to be submitted and run. At this review point, the service sandbox and
Q22 procfs acceptance remained unchanged; the approved service-policy follow-up
below supersedes that state. No suspend/resume, physical monitor reconnect or
browser/video playback certification is implied by these checks.


## Approved user-service accounting policy — November 1, 2026

An explicit Sidecar-only authorization approved PrivateTmp=no, ProtectSystem=no
and PrivateUsers=no while retaining NoNewPrivileges=yes and UMask=0077. The unit
template documents the compatibility tradeoff; no root helper/capability, global
procfs/sysctl setting or other service was changed. Rendered system/user installs
are checked for this security/observability contract with systemd unit validation.

Before deployment, the installed service had a different user namespace from the
desktop caller and inaccessible per-process DRM accounting. After a guarded update
with no active recording, the effective unit retained the approved settings and
shared the desktop user's namespace. Its observed DRM-client count matched an
independent same-account fdinfo read; measured/idle engine metrics and executable
identities became available again. Existing inaccessible processes remained
explicit in coverage; full access to every user process is not claimed.

The updater preserved saved profiles, recording catalog, live launch registrations,
service enablement and tray startup. Local check/test and staged install/removal
checks passed. Read-only live-service sampling and a short CPU/RSS observation
were kept in private local reports rather than added to the public repository.
Firefox engine counters were readable but idle during the observation; this is
not a rendering/video-playback certification. Other hosts, suspend/resume and
physical reconnect still require separate validation. Custom unit overrides can
reintroduce namespace restrictions and must be checked against actual coverage.


## Recording integrity fixes — November 1, 2026

Q29–Q31 cover notes identity, distinct/comparable runs and export interruption.
Desktop model regression switches recordings, preserves unsaved text across catalog
updates and drops deleted/reused identities. Store fixtures reject repeated IDs,
compare sampling and independent explicit setting changes, ignore ephemeral
identity/verification/profile-name differences, normalize launch ordering and
retain unverified missing context. Warnings omit argument/environment values.

`dbus-run-session -- python3 tests/recording_integrity.py` uses private XDG data
and refuses an existing collector. It verifies separate note writes, distinct
run statistics and settings warnings, then applies file-size/core-dump limits
only to its own collector to cause abrupt SIGXFSZ during a synthetic export.
The partial bundle retains its prewritten private marker; restart and a fresh
export succeed, preserving the failed destination and original frame logs.
This requires Linux resource.prlimit but no GPU/display. Make test and CI include it.

Explicit export durability includes file/directory synchronization without adding
per-sample work. Existing large-read tests validate live sampling/RPC/Stop progress;
release CPU/RSS and quiet-tray comparisons use private local observations. Short
uncontrolled runs cannot establish gaming benefit or universal overhead. Native
recordings UI and lifecycle, staged packaging and updater checks validate delivery.
Full hardware suspend/reconnect and playback validation remain separate.

Local `make check`/`make test` passed with 60 Rust tests and the private-bus
integration suites. The recordings screenshot was visually inspected with the
selected run's own notes displayed; cold tray, park/reopen and eleven dashboard
lifecycle cases passed. Staged install/removal and private updater success, active
capture refusal, rollback and interrupted recovery passed. Short paired release
CPU/RSS observations retained quiet-tray window/update/wakeup invariants; measured
collector CPU remained below the normal target in this local sample. Raw reports
and the UI capture remain local. Hosted exact-source checks are not implied.


## Native Wayland Close regression — November 1, 2026

The prior implementation reproduced a false parked state: after native KWin
Close, Sidecar reported tray residency while the compositor retained an undrawn,
non-deleted window. Earlier API/status-based lifecycle tests missed this boundary.
The local eframe fix drains deferred retirement in a windowless cycle before the
application publishes parked state; it does not add a periodic timer.

`python3 tests/wayland_window_lifecycle.py --live --cycles 8` uses an independent
private bus/XDG fixture and short-lived KWin scripts restricted to its child PID.
It verifies no cold-start window, exactly one visible window, zero native windows
after title-bar/API Close, same-process reopen, stopped parked updates and Quit.
It refuses an existing real dashboard; scripts are unloaded and no compositor
settings or real collector state are changed. Raw crash/core details remain local.

A lifecycle unit regression distinguishes invisible/tearing-down from retired and
checks pending Show intent. Existing private startup/tray/dashboard/recording tests
remain required, including X11/software-rendered CI checks. Vendor integrity,
staged source/install/license checks and advisory scanning cover the compatibility
source. Actual KWin acceptance complements CI; it cannot be inferred from status
counters, and other compositors still need their own native checks.

The local full check/test suite passed (61 Rust tests plus private integration).
Eight real KWin native/API Close/reopen cycles passed with independent window
queries. Existing cold tray, six park/activate cycles, rapid reopen, watcher loss,
eleven dashboard lifecycle cases and X11 tray checks passed. Staged packaging
verifies vendor source/license/provenance inclusion; isolated updater success,
refusal/rollback/recovery checks passed. The advisory gate passed with the same
five documented maintenance notices. Release idle observation after warm Close
checks that the process stays alive with no native windows, redraws or telemetry
wakeups; machine-specific reports remain local. No GPU-reset, suspend/reconnect
or gaming-performance certification follows from this window lifecycle fix.

## Reversible normal launches and login — November 2, 2026

Q33 adds opt-in Preview → Apply → Check → Undo to saved iGPU profiles. Keep opens
that setup without automatically changing defaults. Portable Rust fixtures cover
interrupted apply/undo, preserved original bytes, external-edit conflicts,
independent restore-exclusion ownership, unrelated MIME/session keys, ambiguous
INI forms, unsafe paths/symlinks, corrupt journals, upstream/profile drift and
Firefox forwarding with PID reuse/unregistered instances. The suite has 69 Rust
tests; passing fixtures do not prove every application obeys GPU preferences.

`make test` includes `tests/normal_launches.py` on a private bus/XDG tree. Its
`service_fixture` is a fake user manager that starts only an owned collector;
no real systemd service is stopped/started by this test. Local acceptance passed
fresh-preview refusal, ordinary argv/URL routing, startup-conflict detection,
collector/login-style restart, on-demand service start, exclusion/default-handler
persistence, original preservation, recording guards, Undo and conflict recovery.
Successful apply/exec checks need an AMD iGPU and skip explicitly without it.
The existing release EGL placement fixture also passed normal/login routing →
sustained measured iGPU rendering → Undo; it did not claim decoder activity.
Native close/reopen passed six actual KWin cycles plus private-bus dashboard/tray
checks. Staged install/removal and updater success/rollback/recovery passed,
including private routing-journal backup/preservation. Preview/status screenshots
were rendered and inspected locally; captures contain temporary fixture paths.

These checks use temporary launchers/defaults. Real user defaults, Firefox, other
applications and compositor configuration are left alone. A collector restart or
simulated login command is not a real logout/reboot test. Next-login acceptance,
old manually saved sessions, browser codec/video playback and application-owned
startup remain explicit hardware/user checks. Direct script/executable/D-Bus and
alternate desktop-ID launch paths are not universally intercepted.

A clean exact-release resource check used its own process/session bus for ten
seconds: collector 0.50% of one core / 14.7 MiB RSS, cold tray no measurable CPU /
10.6 MiB RSS, zero windows/redraws/telemetry wakeups. Eight local status calls in
the routing fixture peaked at 9.7 ms. These are short uncontrolled desktop
observations, not steady-state memory or worst-case latency bounds; the two-minute
history was still filling, and CPU-tick quantization limits precision. An earlier
35-second observation emitted its complete report (collector 0.60%, 24.3 MiB;
tray 0%, 10.4 MiB) before its outer command wrapper exited 143; that invocation
is not counted as a clean harness pass. The isolated-process-session retry exited
successfully. No raw machine report is published. No route scanner or timer is
added to normal sampling or quiet tray residency.

## Application-first usability pass — November 2, 2026

The primary path is now Applications → Add app → Use iGPU → observe → Undo.
Profiles/trials/recordings remain accessible under Advanced, with data preserved.
New setup cannot create app startup/session-restore changes; old login changes
have a separate retirement operation that leaves normal routing intact.

`make check` and `make test` passed with 72 Rust tests. New private-bus
`tests/app_routing.py` covers pure preview without a saved profile, stale-plan,
recording/maintenance guards, direct argv/URL routing, untouched startup/session/
default-handler files, persistence, existing-profile reuse, and Undo preserving
profiles/recordings. Rust fixtures cover interrupted prepared-profile commits,
v1 legacy journal retirement, idempotence and conflicting external edits.
The release EGL fixture passed automatic separate rendering evidence and rejected
confirmation inherited through a reused profile name with different launch intent.
No decoder confirmation or gaming improvement was inferred from that render test.

Native checks passed 15 private-bus dashboard lifecycle cycles, tray parking/
reopening/startup and six actual KWin retirement cycles. Temporary-prefix packaging
and updater success/refusal/rollback/interruption checks passed. The main view,
app catalog, opaque confirmation and managed card were rendered and inspected;
the README image is a neutral fixture capture, not private user diagnostics.
Installed migration acceptance checks must confirm the normal launcher bytes,
profile settings/PCI target and recording catalog are unchanged while legacy
startup/exclusion ownership is removed. Tests never apply migration to real data
as cleanup; the installed retirement is a separately authorized user action.

One release run used two sequential 15-second observations after three seconds
of warm-up: collector-only 0.53% of one core / 16.3 MiB RSS; Applications-open
collector 0.73% / 25.1 MiB and desktop 0.13% / 121.4 MiB. Reaped diagnostic helpers
bring the latter collector observation to 0.87%. Eight status calls during the
full regression run peaked at 21.2 ms. These short uncontrolled desktop readings
include a growing history/allocator and are not steady-state RAM, worst-case
latency, repeatability bounds or gaming benchmarks. No new quiet-tray polling or
per-sample logging was added; recording catalogs are not polled on main views.
Real reboot, suspend/resume and Firefox video-codec verification were not repeated
as part of this usability pass.

## Applications style refinement — November 3, 2026

The Applications page now uses a centered 880-point content column, aligned app
cards/catalog actions, restrained connection status, and separate rendering/video
panels that stack in narrow windows. Setup intent remains distinct from observed
activity. Undo stays visible, Check shortcut is under Shortcut details, and
conflicts remain visible outside collapsed details. Visuals use the existing
palette and painter-drawn initials; no icon-directory scans, new dependencies,
periodic jobs or collection/storage changes were added.

`make check` and `make test` passed (72 Rust tests plus isolated integration,
security, routing, update-protocol, recording responsiveness/integrity checks).
The primary app-routing fixture passed with real native captures of empty,
catalog, confirmation and managed states at both 1120- and 520-point widths;
these eight images were inspected locally. The narrow capture switch
`IGPU_MAN_SCREENSHOT_NARROW=1` exists only in screenshot-feature builds. The README
capture uses a neutral fixture. No tests assert styling constants/pixel values.
Private-bus dashboard lifecycle (15 cycles), tray Close/reopen and cold tray
startup/fallback passed. Release build and temporary-prefix packaging/install/
removal/source-archive checks passed; systemd unit verification required access
to local verification sockets outside the restricted command sandbox.

No real user app was relaunched by these tests. Actual KWin title-bar retirement,
reboot, suspend/resume, Firefox codecs and runtime overhead were not remeasured
for this visual-only change. Earlier hardware/performance evidence above retains
its stated limits; this pass makes no new resource or gaming-performance claim.

## Focused GPU activity and local labels — November 3, 2026

GPU activity now shows colored usage/power/temperature, charts and compact monitor
connections. Detailed sensors/PCI/render paths and collection/KWin output remain
expandable; partial-coverage warnings and recommendations stay visible. A stale
collector is explicitly labeled as last-known readings. Fixed and automatic axes
retain their existing behavior, and cards stack in narrow windows.

Optional display metadata preserves raw device names and PCI identity. New
`gpu_names` Rust fixtures check exact revision matching across shared Radeon IDs,
conflicting/malformed rows, missing/oversized local data, kernel-name precedence
and CPU-context fallback. A collector fixture checks old sample deserialization
and cache removal after device disappearance or configured-role changes. On the
current machine, the installed local database resolves 744c/cc to Radeon RX 7900 XT;
13c0/cb has no matching entry and uses Ryzen 7 9800X3D graphics as explicit CPU
context, not an asserted GPU model. Raw diagnostic names remain accessible.

`make check` and `make test` passed: 75 Rust tests plus isolated integration,
security, normal/primary routing, update-protocol, recording responsiveness and
interrupted-export checks. Fixed, auto-scale and 520-point narrow native captures
were inspected; the wide captures show both devices with independent axis labels.
Private-bus dashboard lifecycle passed 15 cycles, with paused/missing collector
and recording preservation. Release build and temporary-prefix package/install/
removal/source-archive checks passed. Tray Close/reopen and cold-start/fallback tests also passed before the guarded
installed update. Captures and machine reports
stay local; no user applications are relaunched by these fixtures.

Two sequential collector-only release observations used the same 15-second window
and three-second warm-up on the current uncontrolled desktop: previous release
0.60% of one core / 17.67 MiB RSS; updated release 0.60% / 17.89 MiB. Eight status
CLI calls after each CPU interval peaked at 3.74 ms / 3.84 ms respectively (including
client process startup). `tests/measure_runtime.py` now reports those eight probes
outside the CPU interval. Naming databases load once, with per-device resolution
cached; no recurring name probes or new timers are added. These are single short
observations with CPU-tick quantization, changing desktop load and a growing ring,
not repeatability limits, steady-state RAM, worst-case latency or proof of a speedup.
A concurrent integration run separately peaked at 21.0 ms across eight status calls.
No actual reboot, suspend/resume, KWin title-bar retirement, Firefox codec validation
or gaming-performance comparison was repeated for this change.

## Advanced page refinement — November 3–4, 2026

Advanced Launch settings, Placement experiments and Saved recordings now share
bounded-width cards, muted supporting text and grouped secondary actions. Custom
profiles expose Launch plus More; the editor, rendering/recording switches, normal
routing and Steam commands have dedicated expandable sections. Routing conflicts
remain visible even when its tools are collapsed. Candidate cards retain separate
graphics/video/observation and launch-origin evidence. Trials retain Review →
Verify → Keep gates, and guidance no longer suggests app login startup.

Recordings separate capture, selection, identity-bound notes/files, inspected
results and comparison. Import/export tools are expandable, while deletion still
requires its explicit confirmation. Statistic cells adapt to narrow windows,
retain measured precision, show the exact threshold on hover, and distinguish
unavailable fields from measured zero. No collector/API/storage/schema changes,
new scanning, timers, dependencies or background work were introduced.

`make check` and `make test` passed with 77 Rust tests and the existing private-bus
integration/security/routing/update/recording suites. New headless UI coverage
reproduced and corrected an editor identity mismatch between nested menus and its
collapsing section; the regression checks opening, persistence and body visibility.
A display-boundary regression checks missing/malformed statistics versus actual
zero. `tests/desktop_smoke.py --all` now includes the advanced experiment view
(index 4). All five views were rendered at normal and 520-point widths; profile
editor, selected notes, responsive statistics, normal-routing preview/status and
trial review/verification/recovery captures were inspected locally. Trial captures
allow their scroll animation to settle before capture, rather than treating an
early animation frame as the destination.

The owned EGL placement fixture passed fresh launch, sustained iGPU rendering,
Keep, normal routing, Undo, early-Keep/existing-instance/overwrite/exit guards and
Managed elsewhere. The normal-launch fixture preserved original startup/session/
default-handler files and passed recovery/conflicts. No real user app was restarted.
Private native lifecycle passed 15 cycles, including paused/missing collector and
recording preservation; tray parking/reopen and cold-start/fallback tests passed.
Temporary-prefix packaging/install/removal and source-archive checks passed.

The collector and recording ownership/cadence are unchanged; CPU/RSS, real KWin
title-bar retirement, suspend/resume, reboot, Firefox codecs and gaming performance
were not remeasured for this presentation change. Earlier measured overhead and
hardware observations retain their stated limits. Fixture captures/reports remain
local; installation uses the guarded updater and verifies saved data separately.


Resumed acceptance on November 4 reruns the release checks after the pause. An
extra updater-harness invocation without an isolated bus exposed a fixture safety
gap: it created/stopped a synthetic recording and replaced the tray process with
its temporary copy. Only that recording was removed by exact ID/start/finish/sample
identity, and the installed tray was restored; the real collector and other apps
were not stopped. The harness now self-spawns dbus-run-session by default, refuses
existing Collector/Dashboard/user-manager owners before staging or mutations, and
requires the bus owner's PID to match its owned collector child before proceeding.
A nested occupied-bus regression proves refusal while its profiles/catalog remain
unchanged. Corrected updater success, active-recording refusal, rollback and
interrupted recovery passed on the private bus. The incident is not counted as a
successful isolated run; subsequent installed preservation checks use a fresh
private before/after baseline.

## Compact Applications rows — November 4, 2026

Applications now uses the available width with a fixed 16-point content inset,
rather than a centered 880-point column. Managed apps use short rows with smaller
initials/padding, separate rendering/decoding readings and visible Open app/Undo.
Narrow windows wrap activity below the identity/actions row. Repeated guidance
moves to hover text and Details → Check shortcut; conflicts, interrupted Undo and
other recovery actions stay outside the menu. Requested normal-launch intent
remains separate from recent measured activity. No backend, schema, sampling,
queue, persistence or background-work changes were introduced.

`make check` and `make test` passed with the existing 77 Rust tests and private-bus
integration/security/routing/update/recording checks. No tests assert styling or
pixel constants. The primary app-routing fixture additionally rendered empty,
catalog, confirmation and managed states at normal and 520-point widths; captures
were inspected, including the final narrow Rendering/Video decoding labels. The
README image is the neutral managed-app fixture. Existing routing checks preserve
argv/URL handling, startup/session/default files, profiles/recordings and Undo.
Native lifecycle passed 15 private-bus cycles, including paused/missing collector
and recording preservation; tray Close/reopen and cold-start/fallback tests passed.
Release build and temporary-prefix packaging/install/removal/source-archive checks
passed. Actual user installation uses make update-user and private before/after
fingerprints for routing, launchers, profile settings, catalog and tray startup.

This is a presentation change. Real KWin title-bar retirement, reboot/suspend,
Firefox codecs, gaming performance and runtime CPU/RSS were not remeasured; earlier
hardware/overhead evidence retains its limits. No real user app is restarted by
the isolated fixtures, and collection/storage defaults are unchanged.

## Three-tab dashboard — November 4, 2026

Installed navigation is Applications, GPU Activity and Launch Settings. Advanced
and its placement/recording pages are removed; Launch Settings is directly
accessible and has no Advanced heading. Saved data and optional CLI/API tools
remain compatible. Runtime builds skip retired recording-catalog/pending-trial
polls. Retired widgets remain only as screenshot-feature compatibility fixtures,
without navigation entries, for existing legacy-state regressions.

`make check` and `make test` passed with 78 Rust tests and the private-bus
integration/security/routing/update/recording suites. A navigation regression
checks retired/invalid view normalization; the exact release native check
`IGPU_MAN_BIN_DIR=target/release dbus-run-session -- python3 tests/dashboard_lifecycle.py --runtime-navigation`
checks that retired-view fixture environment settings cannot reopen those pages
in runtime builds. It passed 16 lifecycle cycles including paused/missing
collector, relaunch races and recording preservation. CI runs the release check
before building screenshot fixtures. Screenshot --all now renders only the three
visible tabs; each was inspected and the README's neutral app capture refreshed.
Tray parking/reopen and cold-start/fallback checks passed.

No saved-data migration, GPU placement, collection/storage default or native
lifecycle implementation changes are introduced. Runtime CPU/RSS, actual KWin
title-bar retirement, reboot/suspend, Firefox codecs and gaming performance were
not remeasured; previous hardware/overhead evidence retains its limits. Packaging
uses temporary prefixes, and the installed update uses private preservation
fingerprints and the standard recording/maintenance/window guards.

## Compact Launch Settings — November 4, 2026

Launch Settings now uses the available window width with the same 16-point gutter
as Applications. Short profile rows show name/executable and GPU target side by
side when wide, or name/target together when narrow. Launch/More remain visible;
full executable paths and normal-launch ownership context are available in More,
with a path tooltip on the displayed basename in wide windows. Launch color uses
the requested target GPU, not an observed-placement claim. Managed profile edit/
delete guards, editor identity/drafts, callback payloads and saved fields remain
unchanged. No backend, polling, I/O discovery, persistence or sampling work is added.

`make check` and `make test` passed with the existing 78 Rust tests and isolated
integration/security/routing/update/recording suites. Normal and 520-point narrow
profile rows plus the narrow expanded editor were rendered and inspected. No tests
assert styling constants. Exact-release navigation/native lifecycle passed 16
cycles, including retired-view refusal, relaunch races, paused/missing collector
and recording preservation. Tray Close/reopen, cold-start/fallback, release build
and temporary-prefix packaging/install/removal/source-archive checks passed.
Installation uses make update-user and private before/after fingerprints for
routing/launchers, profiles, recording catalog and tray startup.

This is a presentation-only change. Runtime CPU/RSS, real KWin title-bar retirement,
reboot/suspend, Firefox codecs and gaming performance were not remeasured; prior
hardware/performance observations retain their stated limits. No real user app is
restarted by the fixture checks, and no saved-data migration is required.

## Code-quality hardening — November 4, 2026

- `make check` passes Rust formatting, all-target/all-feature Clippy, documentation
  contracts and Python fixture/packaging syntax compilation. `make test` passes
  80 Rust tests (61 core, 14 desktop, 5 daemon) plus the portable interprocess suite.
- Separate launcher regressions cover existing regular/dangling-symlink destinations,
  private permissions, simultaneous publication (exactly one winner) and temporary
  cleanup. GPU-independent CLI/API checks cover conflicts, unchanged profiles,
  missing/relative/unknown-key requests and the maintenance mutation guard. The
  strict typed `create_launcher` request resolves authoritative saved settings.
- Dashboard queue saturation preserves admitted FIFO work, unwinds only rejected
  normal/trial pending flags, recovers after draining and reports disconnection;
  Quit remains available on the independent lifecycle channel. Visibility hints
  coalesce when work is already queued, preserving event-driven parked behavior.
- `tests/fixture_isolation.py` verifies 16 fixture entry points refuse an occupied
  private bus before mutation. An active synthetic recording, profile/catalog
  identity/metadata and owned collector remain intact; sample counts naturally
  continue increasing. A different live child PID is rejected as collector owner.
  Shared `fixture_bus.py` guards require an explicit session bus, empty protected
  names and the expected spawned collector PID before fixture mutations.
- Release native lifecycle passes 16 clean cycles including ignored retired-view
  environment settings, missing/paused collector and active-recording preservation.
  Release tray lifecycle/startup cover six park/reopen cycles, rapid Show, watcher
  loss/recovery, cold no-display startup, delayed/missing host and parked Quit.
  All three tabs rendered using private synthetic data; Launch Settings inspected.
- Temporary-prefix packaging/install/removal/source-archive checks and updater
  success/refusal/rollback/interrupted-recovery fixtures pass. No application,
  default-handler or startup tests operate on installed user configuration.
- Fresh RustSec audit with cargo-audit 0.22.2 passes the project's vulnerability,
  unsoundness and yanked-crate policy; five existing maintenance notices remain
  visible (paste, ttf-parser, zbus_names, zvariant, zvariant_derive). No dependency
  was upgraded or advisory ignored in this change. Q11's older JSON boundaries
  and Q28's dependency migrations remain follow-ups with acceptance criteria.
- Two separate 15-second release observations, one before and one after, measured
  collector CPU at 0.67% / 0.47% of one core and ending RSS at 16.6 / 16.2 MiB.
  Cold tray had zero measured CPU ticks, zero windows/redraws/telemetry wake-ups,
  and about 10.6 MiB RSS in both. This short uncontrolled sample, with timer
  quantization and differing background activity, establishes no speedup or
  universal CPU bound. It is not a game-performance measurement.
- Native recording responsiveness with a synthetic 127.95 MiB recording and two
  concurrent readers passed: 26 live calls, zero errors, maximum CLI-inclusive
  snapshot latency 2.85 ms; Stop during export 4.17 ms; nine distinct live samples,
  ten persisted samples and three dashboard telemetry updates. These are one-run
  local observations, not latency guarantees.
- Firefox playback/GPU placement, physical monitor reconnect, suspend/resume and
  independent KWin-script window retirement were not repeated for this review.
  The installed dashboard remained parked while fixtures used separate buses/data.

## Documentation and source-publication preparation — November 4, 2026

- Reduce README from 510 to 136 lines and separate setup/removal, everyday tasks
  and CLI instructions into indexed guides. Keep retired UI/startup descriptions
  explicitly historical in validation and consolidate Unreleased around current
  behavior. Review commands, defaults and routing/decoding limits against source.
- Documentation contracts now check public CLI coverage in `docs/cli.md`, while
  README stays a concise front page. Eighteen Markdown documents pass local links,
  index, CLI/API/config/version and distribution contracts; Python syntax checks
  and all-target/all-feature Clippy pass.
- Temporary system/user-prefix installation and source-archive tests pass with all
  new guides. Installed documentation additionally checks local links, including
  the newly shipped changelog, license and source-layout config example. Removal
  retains the existing private-data boundary.
- Full portable suite passes again: 80 Rust tests plus isolated D-Bus integration,
  routing, fixture-ownership, recording-integrity and responsiveness checks. The
  fresh advisory gate passes with the same five maintenance notices. No dependency,
  GPU policy, application route or UI behavior changed in this documentation pass.
- Review the source inventory, synthetic public screenshot, vendored-source hashes
  and credential/personal-path scan before publication. The only path-pattern
  matches were unchanged upstream vendor examples, covered by pinned provenance.
  Logs, recordings, local runtime state, generated builds and exports are excluded.
- Native/hardware evidence remains the dated earlier checks above. GitHub workflow
  configuration and local checks are not a claim that hosted CI has passed; verify
  runs for the actual pushed commit separately. This source push does not create
  a release tag, publish release assets, or submit an AUR/CachyOS package.
