# Everyday use and verification

The main task is **Applications → Add app → Use iGPU → observe → Undo**.
Sidecar changes future explicit launches. It never moves or stops a running app.
For installation, use [setup](setup.md); for commands, use the [CLI reference](cli.md).

## Route an app's usual shortcut

Open Applications, choose Add app, and search installed native desktop launchers.
Review the proposed shortcut change and choose Use iGPU. Sidecar imports the
executable/arguments, saves the settings and publishes a per-user override using
the original desktop ID, name and icon. Packaged originals remain untouched.

Open the app normally. Menu launches, ID-based pins and existing associations to
that ID follow the override. If the app is already running, close it yourself and
reopen when ready; single-instance apps may otherwise reuse their old process.
You do not need to open Sidecar for every app launch.

Routing survives reboot. It does not create app login startup, alter session
restoration, or change your default browser. A direct binary command, custom
script, different desktop ID, direct D-Bus activation or some restored sessions
can bypass the shortcut. Flatpak/terminal/shell launchers need separate handling;
unsupported entries explain why they cannot use quick setup. The preview also
identifies omitted action submenus and activation/hint changes.

## Read the evidence

**Normal launches: iGPU** describes the saved configuration. The separate
**Rendering** and **Video decoding** fields describe observed activity in recent
samples. Configuration alone is not verification.

Allow a few samples after launch and exercise the app. Play a visible video for
decoder evidence. A descriptor merely being open does not establish activity.
Not observed/waiting, stale, inaccessible and idle states carry different meanings;
missing decoder activity is not proof of software decoding. CPU usage or a browser
capability flag alone is also insufficient.

Evidence follows PID plus start time, GPU PCI identity and launch registration.
An unregistered app after a collector restart is not proof it bypassed routing.
Open app uses the managed shortcut; **Details → Check shortcut** inspects its saved
artifacts. Review conflicts and recovery actions before changing anything else.

For native Firefox, fully exit the browser before switching GPU settings.
Sidecar refuses a fresh profile launch while a Firefox-family process is running
and never terminates it automatically. Rendering and video decoding still need
separate checks; `about:support` provides complementary Graphics/Media diagnostics.
Share only relevant, reviewed fields from browser diagnostics. Custom Firefox
forks require their own validation. See [troubleshooting](troubleshooting.md).

## Choose the next app

Applications suggests actionable native apps with recent measured dGPU activity.
Ranking is a way to find candidates, not a promised game-performance gain.
Add app also lists installed launchers, including apps not currently running.

Use **Managed elsewhere** for apps configured by your dev tools, emulator workflow
or another launcher. It suppresses Sidecar's routing suggestions for that
executable; it does not change how the app runs. Expand Managed elsewhere to
restore a suggestion, or use `igpu-man manage /absolute/path/to/app --restore`.

## Undo and recover

**Undo** restores the original local shortcut and any previously owned handler
changes. Outside edits are preserved and shown as conflicts. Automatically created
settings are removed if unchanged; pre-existing/edited profiles and recordings
are retained. An interrupted setup offers **Finish setup**; interrupted Undo can
be retried. Keep the private routing journal until routes are undone.

If an older Sidecar version configured app login startup, the separate retirement
operation removes only its owned changes while retaining normal iGPU routing.
New setup offers no app-startup option. See [legacy login removal](troubleshooting.md#remove-an-earlier-versions-app-login-changes).

## GPU Activity

Orange identifies the iGPU and blue the dGPU. The paired cards show usage, available
power/temperature and connected monitors. Names prefer kernel product labels or
an exact local AMD device/revision match; CPU-based iGPU labels are context, not a
GPU model claim. PCI identity and label provenance stay in Device details.

Charts default to **Fixed 0–100%**. **Auto-scale** fits each GPU's visible two-minute
history with headroom and at least a 0–1% range. Compare the labelled percentages
when axes differ. Scaling only changes the graph, and resets on a fresh dashboard
process. Hover for readings; missing history is not interpolated across long gaps.
Expand Collection & display details for coverage and KWin/monitor diagnostics.

Accessible same-user client activity is partial process coverage, not an additive
whole-device utilization total. Shared-client memory may overlap. Power sensors
may omit CPU/system consumption; their sum is not wall power. Monitor cables,
renderers, decoders and the compositor can involve different GPUs. Cross-GPU
copies and their causes are not directly measured.

## Launch Settings and the tray

Use Launch Settings for custom explicit profiles: executable, argument list, GPU
target and optional Firefox/Vulkan/recording options. Launch uses the saved profile.
More contains edit/delete and separate-launcher actions. Managed normal-launch
profiles require Undo before editing/deletion. Creating a separate shortcut
preserves existing destinations, including symlinks; retries report conflicts.

The dashboard has Applications, GPU Activity and Launch Settings only. Optional
recordings/experiments remain CLI tools; existing data is retained. A recording-enabled
profile can still start a capture: use `igpu-man record stop` to stop it.

Close destroys the window/renderer, retains drafts and the tray, and pauses
polling. Click the tray icon or launch Sidecar again to reopen. **Quit Sidecar**
exits the dashboard/tray; monitoring and launched apps continue independently.
Without a tray host, Close exits; if the host disappears while parked, a fallback
window opens. Sidecar's own optional login startup is described in [setup](setup.md#optional-sidecar-startup).
