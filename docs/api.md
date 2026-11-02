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
Recording read jobs have a cooperative 20-second budget and two concurrent slots;
mutations have eight separate slots. Busy errors mean the request was not queued.
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
| `profiles` | `{}` | `Profile[]` |
| `save_profile` | `Profile` | saved profile with resolved PCI target |
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
additional process information. Unknown/stale metrics are excluded from averages,
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
| `Status` | none | JSON string: `schema_version`, `pid`, `ready`, `quitting`, `visible`, `tray_resident`, `windows_created`, `view`, `updates`, `active_trial`, `tray_available`, `telemetry_monotonic_ns` |

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
