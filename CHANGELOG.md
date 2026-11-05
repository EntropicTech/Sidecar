# Changelog

## Unreleased

### Everyday app routing and interface

- Make Applications → Add app → Use iGPU the main workflow. Import normal launcher
  settings automatically, preserve the desktop identity, show separate recent
  rendering/decoding evidence, and provide per-app Undo with interrupted-setup recovery.
- Remove app login-startup creation and its CLI flag. Retire only earlier owned
  startup/session changes while preserving normal iGPU routing and outside edits.
  Sidecar's own optional tray startup stays independent.
- Use three tabs: Applications, GPU Activity and Launch Settings. Compact app/profile
  rows, keep primary actions visible, and move diagnostics to expandable details.
  Recording/trial pages leave production navigation; CLI tools and saved data remain.
- Prefer cached GPU product names from kernel or exact local AMD device/revision
  matches, with explicit CPU context when an iGPU model is unavailable. Preserve
  raw names, PCI identity, provenance and old recording compatibility.
- Document current setup, everyday tasks, verification and CLI workflows in focused
  guides, with a shorter README, indexed ownership and dated validation history.
  Ship linked changelog/license/example assets and verify installed-document links.

### Correctness, privacy and responsiveness

- Complete native Wayland window retirement before tray parking, with an auditable
  eframe compatibility patch and independent KWin title-bar-close regression.
- Share atomic no-clobber separate-launcher publication through the collector
  writer; preserve existing files/symlinks and keep publication I/O off UI frames.
- Bound dashboard action admission, report busy/stopped workers, and unwind rejected
  pending state while keeping Close/Quit independent.
- Bind notes drafts to recording identity, reject duplicate comparison runs, check
  effective conditions, and durably mark incomplete exports before writing payloads.
- Keep trial settings in bounded private stdin, recovery intents internal, API
  admission before parsing, and diagnostic helper capture bounded during reads.
- Apply the procfs-compatible Sidecar user-service namespace policy, retaining
  NoNewPrivileges/private files and documenting the sandbox tradeoff.
- Cache static version probes and block parked telemetry workers on events. Use
  modest collector executor sizing and a current-thread tray runtime.

### Development and distribution

- Coordinate user updates with recording/maintenance exclusion, private backups,
  atomic installed-file replacement, rollback and interrupted-update recovery.
  Preserve routing journals and live PID/start-identity registrations.
- Generate checksummed source archives and Arch metadata for the public upstream;
  keep AUR/distro submission and release publication separate from source pushes.
- Pin workflow actions, add weekly advisory checks/Dependabot, and retain visible
  dependency maintenance notices without blanket ignores or automatic merges.
- Guard mutating fixtures against occupied buses/wrong collector peers. Self-isolate
  updater tests and cover refusal while an owned synthetic recording remains active.
- Add provenance, routing, recording-integrity, lifecycle, Python syntax and
  documentation coverage checks, plus isolated resource/latency measurements.

## 0.1.0 — initial development baseline

- AMD GPU/application observability and explicit iGPU launch profiles.
- Candidate ranking, installed launcher import, Try → Verify → Keep and exclusions.
- Recordings, markers, MangoHud log import, comparison and exports.
- Sidecar branding, native tray residency and opt-in tray-only login startup.
- Private structured logs, bounded recording I/O and measurable overhead reporting.
