# CLI reference

The technical command name is `igpu-man`. Run commands in your desktop session;
the collector must be available unless using local diagnostics. Get the exact
options for your installed version with `igpu-man --help` or a subcommand's
`--help`. Hidden fixture/update bridges are internal interfaces.

## Status and diagnosis

```sh
igpu-man status
igpu-man status --json
igpu-man status --direct --json
igpu-man doctor
igpu-man logs --component desktop --lines 100
igpu-man logs --component daemon --lines 100
igpu-man-desktop --status
```

`--direct` samples locally without a collector; it is not the installed collector's
recording or registration state. Doctor reports hardware, capability/coverage,
software versions and build identity. Review diagnostics before sharing them.
See [troubleshooting](troubleshooting.md) for safe collection and error recovery.

## Discover and import apps

```sh
igpu-man candidates --json
igpu-man candidates --all
igpu-man launchers
igpu-man profile import chat-side APPLICATION.desktop --gpu igpu
igpu-man profile launcher chat-side
igpu-man manage /absolute/path/to/emulator --elsewhere
igpu-man manage /absolute/path/to/emulator --restore
```

Import saves explicitly requested settings without claiming measured placement.
Applications shows separate identity-bound rendering/decoding evidence for managed
iGPU launches. An unregistered app is not proof of a bypass: collector restarts can
lose live registration. Original launchers and other workflows are preserved.

## Route normal launches

The main **Applications → Add app → Use iGPU** flow imports the installed launcher's
arguments automatically and stores the needed launch settings internally. Preview
is read-only. Confirming Use iGPU saves the settings and a reversible per-user
override with the original desktop ID/name/icon. Packaged files remain untouched;
menu launches, ID-based pins and existing associations to that ID follow the same
route. No default-browser association is changed by quick setup. Desktop D-Bus
activation and discrete-GPU hints are disabled in the override; unsupported action
submenus are omitted, with details available before applying.

Configuration and verification remain separate. Cards show current identity-bound,
sustained rendering and video-engine evidence from recent samples. Play a video
to check decoding; missing decoder evidence does not prove software decoding.
An old or unregistered instance needs a fresh normal launch. Direct executable
commands, another desktop ID, custom scripts, direct D-Bus activation or some
restored sessions can bypass the route. Flatpak/terminal/shell launchers need
separate handling through custom profiles or another workflow. Only actionable
native launcher matches appear in dGPU suggestions; infrastructure is not offered
for app routing. Use GPU Activity or the CLI for diagnostics.

**Check shortcut** checks saved artifacts; **Undo** restores original local
launcher/default-handler values, preserving outside edits as conflicts. Automatically
created settings are removed on Undo if unchanged; pre-existing or edited saved
profiles and all recordings are retained. Interrupted setup has **Finish setup**;
interrupted Undo can be retried. Private recovery data lives in
`$XDG_DATA_HOME/igpu-man/normal-launches.json`; keep it until routes are undone.
New writes use journal schema 2; schema 1 remains readable for retirement/Undo.
Developer updates preserve and privately back up this data.

For custom saved profiles, **Launch Settings** and the CLI remain:

```sh
igpu-man profile normal browser-side --desktop-id firefox.desktop
igpu-man profile normal browser-side --desktop-id firefox.desktop --apply
igpu-man profile check-normal
igpu-man profile undo-normal browser-side
igpu-man profile resume-normal browser-side
# Remove only an earlier version's app-startup changes, keeping iGPU routing:
igpu-man profile remove-login browser-side
```

There is no app `--login` option. Old startup changes can be removed without losing
the iGPU shortcut; removal restores prior startup entries and removes only
Sidecar-added restore exclusions. It does not disable unrelated/user-owned startup.
The optional advanced `--default-browser` setting is explicit and Firefox-specific;
quick setup preserves your existing choice of default browser.

## Explicit launches and custom profiles

```sh
igpu-man profile save browser-side --gpu igpu --firefox -- firefox
igpu-man profile save game-main --gpu dgpu -- /path/to/game
igpu-man profile list
igpu-man profile launcher browser-side
igpu-man launch browser-side
igpu-man run --gpu dgpu -- /path/to/game 'one argument with spaces'
igpu-man profile steam --gpu dgpu
```

Exit Firefox completely before changing its launch GPU. Existing browser
instances receive new-window requests using their original settings; igpu-man
detects Firefox-family processes and refuses that launch instead of terminating
them. Other single-instance applications may similarly reuse an existing
process: verify observed activity. Native Firefox is the initial supported
browser; custom forks need separate validation. Flatpak routing is deferred.

Profiles persist PCI addresses, not unstable `cardN` numbering. Default `igpu` /
`dgpu` aliases are inferred from VRAM size. If classification is incorrect or
ambiguous, select an explicit PCI address from `status`, or define aliases using
the example configuration in [the example configuration](../data/config.example.toml) and restart the service.

Use `--vulkan-only` when a Vulkan application ignores the preferred device order.
This exposes only the chosen Vulkan device; it is an explicit compatibility
option, not a global setting. The Firefox adapter additionally sets
`MOZ_DRM_DEVICE` to the resolved render node. Verify rendering and decoding
individually; device preferences cannot guarantee all application internals.

## Optional recordings and experiments

```sh
igpu-man record start browser-idle
igpu-man mark 'hitch while video started'
igpu-man record stop
igpu-man record list
igpu-man record notes 1 'Same scene, playback smooth, 1440p video'
igpu-man record import 1 /path/to/MangoHud.csv
igpu-man record show 1
igpu-man record compare 1 2 3
igpu-man record export 1 /path/to/new-bundle-directory
igpu-man experiment
```

Recording comparisons require distinct IDs; one recording cannot count as several
runs. Warnings cover recorded environment/monitor metadata, sampling intervals,
stutter threshold and explicit launch settings. GPU-target differences can be the
intended experiment condition; review warnings instead of treating them as proof
of a bad run. Missing settings are labelled unverified. Process IDs, start anchors,
verification timestamps and unrelated saved profiles do not establish a condition
change. Unregistered application settings and playback quality still need notes.

Saved recordings remain available through the commands above. The dashboard no
longer includes recording controls, notes/import/export pages or placement trials.
Recording-enabled custom profiles can still capture data; use `igpu-man record stop`
and the CLI to manage those runs. Existing recordings are not deleted.

Exports start with a private `INCOMPLETE` marker before writing metadata/telemetry.
It is removed only after all bundle files are flushed and synced. A failed or
interrupted bundle stays marked; retry to a new directory after checking the
original recording. Existing output directories are preserved.

Outside an active recording, `mark` saves the preceding two minutes of available
telemetry and the next 30 seconds. Repeated marks extend the capture. In Plasma
System Settings → Keyboard → Shortcuts, add an application/command shortcut for
`igpu-man mark`. No global keyboard hook is installed.

The `igpu-man experiment` command returns an experiment schedule: browser idle, the same video on
the dGPU, and the same video on the iGPU, repeated three times in rotating order.
Each run defaults to 60 seconds of warm-up and 120 seconds of capture. The
schedule is guidance; the utility does not automate gameplay or media playback.
Use the same label for repetitions of a condition. Save video codec, resolution,
game settings, scene, and playback observations in the notes. See
[the experiment guide](experiments.md).

For managed MangoHud capture:

```sh
igpu-man run --gpu dgpu --record --mangohud -- /path/to/game
# Stop telemetry and attach available frame logs when finished:
igpu-man record stop
```

MangoHud logs the first 120 seconds after its one-second startup delay. Temporary
raw logs live under `$XDG_RUNTIME_DIR/igpu-man`; selected logs are stored with the
recording on Stop. Telemetry can continue beyond the frame log. Missing MangoHud
does not affect normal monitoring or recordings. Install `lib32-mangohud` for
32-bit games. Gamescope uses `mangoapp` and is not supported by the managed
wrapper in this release; import its compatible logs separately.

Managed alignment is estimated from the first observed log contents and carries
polling uncertainty. External imports remain unaligned unless you supply
`--anchor-ns`, the collector's monotonic timestamp corresponding to elapsed zero
in that log; optionally supply `--uncertainty-ms`. Never align by file modification
time alone. Only aligned logs appear over telemetry in the timeline. `elapsed`
must be an increasing nanosecond column and `frametime` a positive millisecond
column. Original CSV files are retained.


## Sampling overhead

```sh
igpu-man benchmark --seconds 30
igpu-man benchmark --seconds 30 --interval-ms 250
```

These measure a separate local sampling loop; they are not measurements of the
installed collector plus dashboard, a game benchmark, or proof of a performance
gain. Release process CPU/RSS and concurrent-read checks are described in
[validation](validation.md).

## Window and tray control

```sh
igpu-man-desktop
igpu-man-desktop --tray
igpu-man-desktop --status
igpu-man-desktop --close
igpu-man-desktop --quit
```

Ordinary launch opens the dashboard or requests Show from its existing instance.
`--tray` starts without a window and leaves an existing instance unchanged. Close
parks with a tray available; Quit exits the dashboard/tray. Neither stops the
collector or other applications. Help/version/error checks work without a display,
D-Bus connection or recording database. See [setup](setup.md) for optional startup.
