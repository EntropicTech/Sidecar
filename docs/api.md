# Local API v1

Session D-Bus name: `io.github.igpu_man.Collector`.
Object: `/io/github/igpu_man/Collector`.
Interface: `io.github.igpu_man.Collector1`.

`ApiVersion` is a read-only `u32` property.
`Request(operation: string, payload: string) -> string` accepts JSON and returns
JSON. Failures are D-Bus Failed errors with a user-readable explanation. Clients
must check the snapshot `api_version`. Only the local user's session bus is used.
The Rust client establishes a fresh connection per transaction to recover after
daemon restarts. There is no TCP listener or privileged helper.

Object payloads may include `_request_id`: up to 96 ASCII digits/hyphens. The Rust
client supplies `PID-monotonic_ns-sequence`; the daemon logs it without logging
the payload. Ordinary calls time out after three seconds; `import`, `export`,
`samples`, `samples_page`, `timeline`, `frames`, `detail`, `compare`, `start`, `stop`, and `mark` use 30 seconds. Mutations are not
automatically retried: a timeout leaves an uncertain outcome. The request envelope
is limited to 70 MiB for import and 1 MiB otherwise; raw frame logs to 64 MiB and
configuration to 1 MiB. CSV limits are one million frames/256 columns; stored frame
encoding is capped at 128 MiB. Duplicate timing columns are rejected as ambiguous.
Live requests (including capabilities) have four concurrent slots, recording reads
have two, and mutations eight. Admission precedes blocking JSON parsing and
state cloning. Excess calls return D-Bus LimitsExceeded immediately; busy requests
are not queued. Read/parse permits remain held until blocking work finishes, even
if the caller cancels. Recording reads retain a cooperative 20-second budget.
Timeline/frame arrays are limited to 64 MiB of stored JSON; export larger recordings.
Read connections are query-only WAL snapshots, so a concurrent recording may have
more recent data than an in-progress detail/compare/export result.

| Operation | Payload | Result |
| --- | --- | --- |
| `capabilities` | `{}` | API version and additive feature names; probe before relying on guarded Stop |
| `snapshot` | `{}` | `Snapshot` |
| `candidates` | `{}` | ranked `Candidate[]` with 60-second activity and coverage |
| `launchers` | `{}` | installed XDG launchers, parsed profiles or unsupported reasons |
| `management` | `{}` | executable paths marked Managed elsewhere |
| `set_management` | `{executable, managed_elsewhere: bool}` | persisted exclusion paths |
| `verify_trial` | `{process: {pid, start_ticks}}` | separate rendering/decoding/other-engine verification |
| `keep_trial` | `{process, name, create_launcher: bool, cli_path?}` | saved profile confirmation, launcher path, verification |
| `prepare_update` | `{token: 64 hex characters}` | bounded five-minute mutation lease, live launch specs, schema versions; refuses active/stopping recording |
| `cancel_update` | `{token}` | release matching lease; other nonces rejected |
| `pending_trials` | `{}` | unkept `LaunchRegistration[]`; exact explicit launch settings retained while tracked |
| `discard_trial` | `{process}` | boolean; removes unkept setup, retains placement attribution and leaves app running |
| `app_route_preview` | `{desktop_id, cli_path}` | read-only primary app setup; imports original settings and selects the single iGPU; no profile saved |
| `app_route_apply` | `{setup, token}` | create/reuse compatible settings and publish the reviewed normal-launch route; never creates startup entries |
| `normal_launch_remove_login` | `{profile}` | retire only old app-startup/exclusion changes, preserving normal routing and other settings |
| `normal_launch_preview` | `{profile, desktop_id, cli_path, login?: false, default_browser}` | strict setup preview, freshness token, change labels and limitations; no writes |
| `normal_launch_status` | `{}` | saved route phase, launcher/login/defaults/exclusion checks and conflicts; no raw undo content |
| `normal_launch_apply` | `{setup, token}` | revalidate preview, journal undo data, apply routing for a saved iGPU profile |
| `normal_launch_resume` | `{profile}` | finish an interrupted installation; conflicting external edits are preserved |
| `normal_launch_undo` | `{profile}` | restore owned changes, preserve unrelated settings and keep journal on conflict |
| `profiles` | `{}` | `Profile[]` |
| `save_profile` | `Profile` | saved profile with resolved PCI target |
| `create_launcher` | `{name, cli_path}` | new separate launcher path; absolute CLI path, existing destinations preserved |
| `delete_profile` | `{name}` | boolean |
| `register_launch` | `LaunchRegistration` | boolean |
| `start` | `{label}` | recording ID |
| `stop` | `{expected_id?: integer or null}` | recording ID; an integer refuses to stop a different active ID; omitted/null means explicit unconditional Stop |
| `mark` | `{label}` | recording ID and optional capture deadline |
| `recordings` | `{}` | recording metadata list |
| `detail` | `{id}` | metadata, frame summaries, GPU sample means |
| `samples` | `{id}` | legacy telemetry snapshots, 64 MiB materialization limit |
| `samples_page` | `{id, after?: {monotonic_ns,row_id}, through_ns?, limit?: 1..128}` | stopped-recording page, max 4 MiB raw JSON; `next_cursor`, `through_ns` |
| `timeline` | `{id}` | compact GPU-only samples and cached peak frame series |
| `frames` | `{id}` | stored frame logs with alignment metadata |
| `notes` | `{id, notes}` | boolean |
| `compare` | `{ids: [id, ...]}` | per-log summaries, ranges, mismatch warnings |
| `import` | `{id, filename, raw, anchor_ns?, uncertainty_ms?}` | boolean |
| `export` | `{id, destination}` | boolean; creates a new directory |
| `delete_recording` | `{id}` | boolean; active recordings cannot be deleted |
| `experiment` | `{}` | rotating nine-run schedule |

Operation names/classes are typed in `crates/core/src/api.rs`; payload/domain types are in `crates/core/src/model.rs`. The external v1 string/JSON wire format is unchanged. Process identity is PID plus
start ticks. GPU identity is PCI address. Monotonic times are nanoseconds from
Linux CLOCK_MONOTONIC and meaningful within the recorded boot (metadata includes
boot ID). Human timestamps use UTC Unix milliseconds. Metrics carry value,
unit, source, and availability; null must not be treated as zero.

Launch wrappers register their identity before exec so they preserve the desktop
environment without transmitting it to the collector. An observed descendant
retains registration after its launcher exits, using PID+start-time membership.
A process that forks and exits entirely between samples may be unobservable.

Application snapshots additionally expose `launch_id` and `profile_name` when
registered, including after a launcher exits while observed descendants remain.
Candidates are derived from the bounded collector ring; querying does not collect
additional process information. Each candidate adds `launch_path`: `kind`,
`registration` (PID/start identity), `registered_profile`, `matching_profiles`,
and `explanation`. Kinds are `registered_profile`, `registered_trial`,
`registered_command`, `unregistered`, `managed_elsewhere`, `infrastructure`,
`unknown`. Older replies default to unknown. Registration is evidence of an
explicit Sidecar launch, not of a particular desktop entry or successful GPU
placement. Multiple matching saved targets leave `saved_target` unspecified
unless registration establishes a target. Exclusions suppress profile suggestions.
Capabilities advertise `launch_path_audit` and `cached_software_versions`.
Unknown/stale metrics are excluded from averages,
and observation coverage is explicit. Sampling gaps do not count as sustained work.

Trials use an explicit CLI exec wrapper without saving a profile. Verification
only considers the matching launch identity since its registered monotonic anchor
and within the last 15 seconds. Target activity needs three active samples spanning
two seconds; measured activity on other GPUs is reported separately and blocks
Keep. An `enc`/unified video-engine name alone is not reported as verified decode.
Keep rechecks live identity, fresh accounting and evidence in the backend, rejects
existing profile/launcher names, and commits a pending-launcher intent with the profile before atomic no-clobber publication. Startup reconciles interrupted intents; conflicting launchers are preserved. A collector restart discards live trial identity; the trial must be repeated.
The saved verification is historical evidence, not a promise about future streams.
Dashboard exits do not discard collector-owned trial settings. Resume starts with
no cached verification and obtains fresh evidence. Saved profiles cannot be
discarded as trials. Feature names currently include `guarded_stop`, `request_ids`,
`pending_trials`, and `structured_logging`.
`isolated_recording_io` identifies the sampler/recording separation. Older v1 collectors may not expose
`capabilities`. The client fails safely instead of sending an identity-guarded
Stop to a collector that would ignore the guard.

Profile additions (`environment`, `working_directory`, `desktop`, `verification`)
and application identity fields have serde defaults for existing v1 configurations
and recordings. Launcher file/URL field positions survive import and generated
launchers forward inputs after `--`; no shell evaluation is involved.

Database schema v1 is initialized on first start. Future breaking schema changes
must add an explicit migration/version gate. Live records left by a crash become
`interrupted` on startup. Exports contain `metadata.json`, `telemetry.jsonl`,
`markers.json`, original `frames-N.csv`, and separate alignment JSON files.

## Dashboard control v1

Name `io.github.igpu_man.Dashboard`; object `/io/github/igpu_man/Dashboard`;
interface `io.github.igpu_man.Dashboard1`. Ownership is exclusive with no queue or
replacement. A new launch forwards Show to its existing owner. Status probes do
not claim ownership. This interface has its own connection/control queue and a
two-second client deadline, independent of telemetry requests.

| Method | Inputs | Result |
| --- | --- | --- |
| `Show` | `applications: bool` | boolean: queued; optionally selects Applications, requests attention/focus subject to compositor policy |
| `Close` | none | boolean: queued; parks window when tray available; otherwise exits dashboard/tray |
| `Quit` | none | boolean: queued; exits dashboard/tray |
| `Status` | none | JSON string: `schema_version`, `pid`, `ready`, `quitting`, `visible`, `tray_resident`, `windows_created`, `view`, `updates`, `telemetry_wakeups`, `active_trial`, `tray_available`, `telemetry_monotonic_ns` |

Control acknowledgments mean accepted, not that process exit is complete. The
CLI's `--status` helper reports `{"running":false}` when the name is absent.
Neither Close nor Quit stops the collector or its recording/workloads. Diagnostic
logs have a separate schema described in [engineering-standards.md](engineering-standards.md).
`telemetry_monotonic_ns` is the latest snapshot received by the dashboard; it
distinguishes live collection refresh from a window that only continues drawing.
Stop stops admission of new selected samples, drains earlier FIFO samples, and
returns its ID only after persistence. Snapshots retain the active ID and a
stopping diagnostic during draining; an API timeout still has an uncertain outcome.


## Recording display and compatibility

`compact_recording_timeline` identifies the GPU-only projection. The dashboard
falls back to legacy sample/frame reads against an older collector. The compact
view omits process trees and original CSV, caps GPU points at 10,000 and original
frames at one million across at most 128 logs. Frame series contain at most 3,000
cached peak-bucket points across those logs, with `total_frames` and `aggregation`
describing reduction. Statistics/export retain every original frame. Raw sample
pages require a stopped/interrupted recording: pass `next_cursor` into `after`
and retain `through_ns` until the cursor is null. Timestamp plus row ID preserves
duplicate timestamps; a deleted recording returns an error rather than a partial page.

Keep can return `launcher_warning` after its profile committed successfully but
publication/intent cleanup is pending. That profile remains usable through
Profiles/CLI; do not retry as though nothing was saved. Versioned pending intents
are reconciled once at startup and after Keep; no repair polling is added.

Dashboard Status also contains `source_sha256` and `worker_health` (telemetry state,
recording state, backend integrity). Unhealthy trial state is unavailable and its
telemetry timestamp is zero. The fallback view honors Close/Quit without reading
poisoned data. Collector capabilities contains constant `build` metadata.

Close retains the tray and in-memory model when available; clients requiring
process exit must use Quit. Show/Activate recreates the native window in the same
process. While parked, `visible` is false, `tray_resident` is true, UI `updates`
stops advancing and `telemetry_monotonic_ns` is zero. `view` is the last selected
view index. Worker-health/trial fields reflect the most recent UI publication;
collector state remains authoritative. An accepted Show is retained across close
teardown, but Quit rejects further Show. Watcher disappearance reopens the window
and watcher return re-registers the icon. No method signature changed.

The desktop CLI supports `--tray` for cold login startup; it is mutually exclusive
with other modes and does nothing to an existing owner. Initial tray status has
`windows_created: 0`, `visible: false`, and `updates: 0` even without a display.
`ready` means the resident lifecycle is initialized, not that a window exists.
After tray registration `tray_available` and `tray_resident` become true. A late
watcher can register within the 15-second startup grace without showing a window.
The window count increases on context attachment, making cold startup verifiable.
Control replies are drained for at most one second during process exit.

`coordinated_updates` advertises update leases. The writer checks lease admission
both before queueing and before executing, covering already queued mutations.
Only matching `_update_token` on register_launch permits identity-bound restoration;
that reserved field is removed before strict domain decoding. It is never logged.
Read-only/live sampling remains available. `IGPU_MAN_UPDATE_TOKEN` is an internal
validated service-start nonce for carrying the lease across restart; it expires
within five minutes if the updater fails. Package installation never sets it.

The dashboard `telemetry_wakeups` counter increments when its telemetry worker
receives an event or visible-mode timeout. It remains stable while cold/parked
without events; status probes do not wake that worker. Visibility transitions
explicitly wake it for fresh polling on reopen. Software-version diagnostics
are cached up to five minutes; executable/package-database identity changes are
checked on the independent 30-second display-diagnostics cadence.

Non-null `Profile.pending_launcher` is internal recovery state and is rejected by
`save_profile` and `register_launch`, including nested launch specifications.
The hidden trial CLI receives at most 1 MiB of JSON over stdin, never argv.


## Recording integrity

`compare` rejects repeated IDs rather than counting one recording multiple times.
The reply shape is unchanged: `mismatch_warnings` now also covers sampling
intervals/stutter threshold and stable explicit launch settings. It ignores process
IDs, anchors, verification timestamps, profile names and launcher presentation.
It retains multiplicity of independent launches and normalizes their ordering.
Missing environment/sampling/launch context produces unverified-comparability
warnings. Saved-profile differences are fallback context only when both runs lack
complete effective launch specifications. Warnings name categories and recording
IDs without echoing profile argument/environment values. Deliberate GPU changes
remain allowed and are reported for interpretation. See [experiments](experiments.md).

`export` creates and syncs an INCOMPLETE marker before bundle data. All output files
and directory entries are synced before marker removal; successful return follows
completion synchronization. Errors or abrupt collector exit retain an incomplete
bundle. A fresh destination is required on retry; existing directories are never
replaced. A timed-out export still has an uncertain outcome; inspect the marker
and collector state before retrying. This adds work only to explicit export jobs.


Dashboard Status adds `native_window_active`: true from native attachment until
post-teardown cleanup finishes. `tray_resident` requires retirement, an available
tray, no Quit and no pending Show; visible=false alone is not retirement. Cold
tray startup has no native window. `dashboard.window_retired` logs completed
cleanup; `dashboard.parked` follows when the resident loop waits for tray input.

## Normal-launch routing

The additive `normal_launches` capability identifies this workflow. Preview/status
are bounded read jobs, off the collector lock; apply/resume/undo use writer
admission/FIFO and refuse active recordings or maintenance leases. Setup is strict:
clients cannot supply target paths, file contents or undo/recovery operations.
Desktop IDs are validated and resolved from installed entries; executable identity
must match the saved profile. Apply requires a current iGPU target and a fresh
preview token including profile/source/current artifact state. Tokens are change
detection, not authorization or signatures. Only local account access is trusted.

Status phases are `installing`, `active`, `undoing`, and legacy `removing_login`. Configured artifacts are not
placement evidence. `verify_trial` also verifies an existing saved registration;
its sustained rendering and decoding evidence remains identity-bound and separate.
Checks are on demand; external changes do not trigger silent automatic repair.
`launch --normal` can forward browser inputs only when all live Firefox-family
PID/start identities belong to the matching registered profile/GPU. It refuses
unknown instances, including after uncoordinated collector restarts.

Managed profiles cannot be edited/deleted or marked Managed elsewhere before
Undo. The normal-launch journal is private internal state, never a public RPC
payload. Errors and events omit its original/generated content.


## Primary app workflow and automatic evidence

`simple_app_routing` advertises `app_route_preview/apply`. Preview uses two bounded
read slots and imports the original launcher's settings without writing a profile.
Apply uses mutation admission/FIFO, refuses recordings/maintenance and commits
private prepared settings before publishing derived launchers. No caller-authored
profiles, GPU selector, artifacts or recovery operations are accepted by these
methods. A single discovered iGPU is required; custom GPU selection stays advanced.

`candidates` adds optional `verification`, computed from the existing last-15-second
ring for saved iGPU registrations. No process scanning/RPC is added for verification.
Evidence requires live PID/start identities and matching effective registered
settings; a reused settings name cannot inherit another intent's confirmation.
Rendering and decoding remain independent. Unregistered, idle, unavailable and
stale states must not be shown as successful verification.

Normal setup rejects `login:true`; only legacy retirement/Undo can edit autostart
or session exclusions. `normal_launch_remove_login` has a durable removing phase;
conflicts preserve edits and undo data. Journal schema 2 includes optional prepared
settings and ownership; schema 1 is readable but old binaries cannot rewrite v2.
Undo removes unchanged auto-created settings and retains pre-existing/edited ones.
Runtime dashboard view IDs are 0=GPU Activity, 1=Applications (default), and
2=Launch Settings. Retired IDs 3/4 are restricted to screenshot-feature compatibility
fixtures; normal builds normalize retired/invalid selections to Applications.
The control API gains no view-selection method. Recording/placement collector
APIs and the CLI remain compatible, but those pages are absent from the dashboard.


## GPU presentation metadata

`Gpu` adds optional `display_name` and `display_name_source` strings. They default
to null when reading older snapshots/recordings. `name` remains the existing raw
diagnostic name and `pci` remains authoritative identity. Clients may prefer a
non-null display label, but must never route by it. Sources describe sysfs product
metadata, an exact local libdrm device/revision match, or host CPU context when an
iGPU model is unavailable. CPU context is not a GPU model or measured capability.
No operation signatures, sampling defaults or persistence schemas change.

### Separate launcher publication

`create_launcher` resolves the saved profile by name on the collector's serialized
writer, outside the sampler lock. It is a mutation subject to admission and update
maintenance exclusion. It does not edit profiles, normal-launch routing or defaults.
Publication syncs a private temporary file, then atomically links it without
replacement; existing files and symlinks are errors, including identical shortcuts.
A parent-directory sync failure after publication is a logged durability warning.
On an uncertain timeout, inspect the destination before retrying. CLI and dashboard
use this same endpoint rather than writing their own files.
