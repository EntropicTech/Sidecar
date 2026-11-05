# Engineering standards

These are the project baseline for human and AI contributions. Apply them to new
work and fix existing violations with focused regression coverage. They do not
imply that every historical implementation already satisfies every future target;
the [review](code-review.md) tracks remaining gaps.

## Errors and recovery

1. Return `Result` at I/O, parsing, IPC, persistence and process boundaries. Add
   operation context while preserving the cause. Malformed inputs must not panic.
2. Expected missing hardware/capabilities are availability states. Do not hide
   permission failures, pretend stale data is fresh, or substitute zero for unknown.
3. Treat broken state invariants and poisoned authoritative state as fatal to the
   affected process. Log the failure; preserve the database for recovery. Do not
   broadly swallow panics and continue with partially mutated state.
4. Handle cleanup failures separately from the initiating error. Recording cleanup
   must name the expected session so it cannot stop a newer recording.
5. Supervise background tasks. No ignored JoinError or retry loop that can keep a
   failed sampler looking alive. UI Close/Quit must not wait for a telemetry RPC.
6. A request timeout is an uncertain outcome, not proof that a mutation did not
   happen. Inspect state before retrying; never automatically retry mutations.
   Idempotent dashboard Show may retry when its old owner is exiting. Once Quit
   is queued, shutdown state is monotonic and must reject Show immediately.

Ordinary collector calls have a three-second deadline. Large recording operations
have 30 seconds. Dashboard control has two seconds. Requests have a 70 MiB envelope
limit for imports, 1 MiB for other operations; frame logs have a 64 MiB raw limit,
one million frames, 256 columns and a 128 MiB stored encoding limit. Configuration has a 1 MiB limit. The envelope
can reject an encoded input even when its raw data fits. Reads must enforce bounds
while reading, not only check file metadata before a potentially growing file.

Recording readers have a cooperative 20-second scan budget, two concurrent jobs,
and query-only SQLite transactions. Large exports/statistics stream rows; legacy
materialized timeline/frame reads cap stored JSON at 64 MiB. Marker responses cap
at 16 MiB and catalog metadata at 64 MiB. Fail explicitly at limits; never silently
truncate evidence. A bounded FIFO preserves sample/Stop ordering and reserves
control capacity. Queue exhaustion must be visible and never produce a falsely
complete recording. Log writer-delay/recovery transitions, not every delayed tick.

## Performance discipline

Keep normal/recording sampling defaults at 1 Hz/4 Hz. Share immutable snapshots,
sleep idle workers, bound concurrency and buffers, and perform heavy work only
on explicit recording requests. Optimize measured hot paths before adding caches
or periodic work. Target collector-only CPU below 1% of one core under the normal
rate. Compare release CPU/RSS before and after under matching workloads; record
wall time, sample gaps and live-request latency under heavy recording I/O. Report
repeatability/occlusion limits, and do not call a single run a general guarantee.

## Logging contract

Location: `$XDG_STATE_HOME/igpu-man/logs`, default `~/.local/state/igpu-man/logs`.
Directory mode is 0700, file mode 0600. Files are per component and PID. Each file
rotates at 5 MiB with three backups; retention keeps 32 inactive files and never
prunes a file whose process is still alive. Active files are in addition to that
retention count. An event is capped at 64 KiB. Failure to open/write logs falls
back to structured stderr output rather than preventing normal app operation.

Every JSONL event has:

| Field | Meaning |
| --- | --- |
| `schema_version` | Log schema, currently 1 |
| `unix_ms` | Wall-clock timestamp for humans/cross-process inspection |
| `monotonic_ns`, `boot_id` | Within-boot ordering and timing |
| `source_sha256` | Rust-source/dependency identity of the emitting build |
| `component`, `pid` | Process emitting the event |
| `level`, `event` | Severity and stable event name |
| `fields` | Allowlisted structured diagnostic context |

Examples: `process.started`, `dashboard.closing`, `api.request_completed`,
`recording.started`, `recording.stopped`, `launch.registered`, `cli.failed`,
`daemon.failed`, `runtime.panic`. Do not rename events or fields casually: AI and
human runbooks rely on them. Additive fields are compatible; breaking changes
need a schema change and migration/readability plan.

Use info for lifecycle and explicit mutations, warn for recoverable failures or
slow operations, error for lost work/fatal failures, debug for optional details.
Default logs must not grow once per telemetry sample. Log repeated connection
failures on transitions and log recovery. `IGPU_MAN_LOG=debug` enables the app's
debug messages, not unrestricted dependency payload tracing.

Never log RPC payloads, complete argv/environments, profile environment values,
configuration excerpts, browser history/URLs/page contents, application raw output,
or user notes. Log request ID, operation, duration, outcome, recording ID, process
identity, GPU PCI identity and counts when needed. TOML/JSON parser source excerpts
are omitted, URLs are redacted, and library messages are bounded. Redaction is
defense in depth, not permission to pass sensitive data to the logger.

Panics produce a location and backtrace without copying the arbitrary panic
payload. Logging itself must not recursively panic while handling another failure.

## Persistence and launch safety

- Config saves use an unpredictable exclusive temporary file in the destination
  directory, private permissions, file sync, and atomic rename. A parent-directory
  sync failure after rename is logged as a durability warning: the write was
  applied and must not be misreported as safe to roll back.
- Preserve existing names. Commit the profile and pending launcher intent, publish
  atomically without replacement, then clear the intent. Reconcile interrupted
  publication and preserve conflicts. Report partial publication as a warning on
  a usable saved profile; do not misreport it as an unapplied operation.
- SQLite writes must respect the configured budget. Recovery must distinguish
  completed and interrupted recordings. Never silently delete recordings for space.
- Pass argv directly, never concatenate shell strings. Resolve GPUs by PCI each
  launch. Capture only explicit launch settings; never scrape complete process
  environments or copy arbitrary existing process arguments into a profile.

## Review and validation bar

Review correctness, privacy, resource bounds, race conditions, shutdown, observability,
compatibility and documentation—not only happy-path output. Use minimal fixtures
for parser changes and real process/D-Bus integration for lifecycle changes.
Separate display-dependent and GPU-dependent tests from portable checks.

Required delivery evidence: formatting, all-feature Clippy, appropriate unit and
integration tests, documentation checks, and native captures for visual changes.
Record untested hardware scenarios honestly. Do not infer gaming benefit from a
GPU counter, or claim repeatability from a single overhead observation.

Follow the documentation matrix in [AGENTS.md](../AGENTS.md). Public interfaces,
defaults, error paths and instructions must ship with the same change. CI checks
links, source/API coverage, the config example and install/distribution coverage;
semantic accuracy still needs human or AI review.

## Compatibility, worker recovery and build identity

Reject unknown config/specification keys; preserve omitted-field v1 defaults.
Parser diagnostics identify safe byte ranges without copying configuration text.
Native help/version/error handling must precede graphics, D-Bus and storage setup.

Observe worker health without retry loops. Never recover through a poisoned lock.
The GUI may abandon a frame that raced with backend poisoning and show a recovery
view; it must not resume product logic on that state. Unrelated panics remain
fatal. Verify failure, clean Close, and fresh reopen against an independent collector.

Keep source/build identity static. Hash artifacts only for explicit diagnostics,
build or installation; do not capture raw build flags or complete environments.
Release manifests distinguish exact binaries; source IDs alone are not a claim of
bit-for-bit reproducibility across toolchains/options.

Compact timelines are display projections. Label peak-bucket reduction, keep full
statistics/exports, and bound render work independently of total frame count.
Raw paging uses timestamp/row-ID cursors and a fixed endpoint on stopped recordings.

Tray residency must not depend on unsupported native visibility operations.
Destroy the window/renderer, retain only Rust state and the tray, pause dashboard
polling, detach repaint callbacks, and block waiting for control. Rebind context
on reopen. Test both missing-tray exit and watcher loss, same-process draft/view
retention, stopped parked updates, rapid Close/Show ordering and parked Quit.
Neither window Close nor desktop Quit may stop an unrelated collector recording.

Cold tray startup must not create or briefly map a native window; expose a
window-creation counter so tests can prove this without relying on timing. Keep
login setup opt-in, idempotent and user-scoped. Support delayed/missing hosts and
Quit before first window creation. Drain replies with a bounded shutdown deadline,
not an arbitrary fixed sleep. Do not restart an active recording to enable login.

Updates must stage/verify complete cohorts before stopping anything, refuse active
recordings and close the start-during-restart race with a bounded maintenance lease.
Do not scrape process argv/environments or log lease/payload/config contents.
Rollback restores installed programs; incompatible data migrations require explicit
review. Public packages use pinned sources, frozen dependency builds, current
SRCINFO, reviewed metadata and isolated package checks. Never equate configured CI
or a local successful build with repository acceptance or hosted CI success.


Launch-origin explanations require registration evidence. Executable/profile
matches alone must not imply a desktop shortcut, a bypass, or successful routing.
Reuse existing accounting and respect exclusions. Cache static diagnostic probes
outside sampling; preserve explicit freshness limits and missing-tool recovery.
Parked telemetry must block on events and be explicitly woken when visibility
changes. Test cold start, repeated park/reopen and quiet wake-up counters.


Choose async executor sizes for the actual I/O/control workload. A dedicated
thread that continuously drives a current-thread runtime must keep driving it
while parked. Do not shrink pools by mutating the application's inherited global
runtime environment. Validate real tray/IPC progress, sampling under concurrent
recording reads, failure recovery, CPU/RSS and OS thread counts; fewer threads do
not by themselves prove lower CPU, lower physical RAM or better game frametimes.
Dependency-owned runtimes must be included when explaining total thread counts.


## Security maintenance

Apply [security boundaries](security.md): transmit private trial settings through
bounded stdin, reject caller-supplied recovery intents, and acquire API admission
before parsing or cloning state. Retain permits in blocking jobs after caller
cancellation. Diagnostic capture must enforce byte/time limits during reading,
without growing temporary files or logging raw output.

Pin external workflow actions to full upstream commit SHAs; keep checkout
credentials out of build working trees. Run `make security-check` before releases.
CI runs advisory checks weekly and on changes; vulnerabilities, unsound notices
and yanked crates fail, maintenance notices remain visible and tracked. Do not
hide notices with blanket ignores or automatically merge dependency updates.


Collector unit hardening must preserve its authorized cross-process procfs access.
The reviewed compatibility policy disables PrivateTmp/ProtectSystem/PrivateUsers
only for Sidecar's user service, retaining NoNewPrivileges, restrictive umask and
private data. Never silently reintroduce namespaces or compensate with ptrace/root
privileges. Verify actual unit properties and cross-process coverage after updates;
standalone fixture success alone is insufficient. See [security](security.md).


Notes editors must bind drafts and save payloads to recording identity; inspecting
another run or refreshing a catalog cannot substitute its notes. Preserve unsaved
drafts while parked and prune deleted/replaced identities. Reject duplicate run
IDs in comparisons; distinguish missing settings from matching settings, normalize
ephemeral launch metadata, and do not echo sensitive values in mismatch warnings.

Mark export bundles incomplete before any payload is written, sync the marker,
and remove it only after synced completion. Error-only cleanup cannot cover abrupt
exit. Regression coverage must include a real owned-process interruption with
private data, successful retry and preserved destination conflicts. Keep durability
work outside the sampler and document uncertain timeout outcomes.


Native Close acceptance must independently verify compositor window removal, not
only internal visibility/tray flags. Detach repaint callbacks before tearing down
GL, and do not report residency until deferred native retirement has completed.
Exercise title-bar and API Close, same-process reopen, parked idle behavior and
Quit. A windowless one-shot cleanup cycle must not become periodic polling.

Checked-in compatibility sources require an upstream archive checksum, unchanged
file hashes, exact reversible diff, licenses, build-identity/distribution coverage
and a removal criterion. Verify before compilation; never silently modify Cargo
registry/system library files. Keep crash/core diagnostics local and narrowly
filter journals by owned process/component.

## Desktop routing changes

Normal-launch setup must have a fresh preview, durable private undo intent and
explicit conflict handling. Never accept arbitrary artifact text/paths from RPCs,
follow destination symlinks, restore a whole MIME/session configuration over
unrelated edits, or delete recovery state after partial failure. Fail visibly on
unknown journal schema/corruption. Normal routing emits only `normal_launch.applied`
and `normal_launch.undone` events with allowlisted option flags; never log original
launcher bytes, handler values, profile payloads or URL inputs. Desktop launch
failure notifications use fixed text. Preview/Apply/Resume/Undo use bounded jobs
and calls; no route polling is added while parked. Configuration/placement claims
stay distinct. New routes require opt-in; deployment is not permission to change
all saved applications' defaults.


## Primary usability contract

The default task is Applications → Add app → Use iGPU → observe → Undo. Never
require named profiles, a trial, a recording or a second launcher setup for the
ordinary supported case. Keep custom profiles in Launch Settings and preserve optional CLI data. New app setup must not create startup entries or edit session restore;
legacy retirement alone may reverse earlier owned changes. Sidecar tray startup
is independent. Keep configured intent distinct from observed rendering/decoding,
and reject inherited automatic confirmation when a settings name changes intent.
Do not add executable resolution, full launcher/profile copies or recording catalog
polls to main-view redraws. Preserve preview purity and prepared-profile recovery.

Applications styling should emphasize setup actions and measured evidence separately.
Keep Undo discoverable, surface conflicts outside collapsed details, and make narrow
layouts readable. Use existing data and embedded/painter visuals; a style pass must
not add icon-directory scans, animation timers or telemetry work. Inspect fixture
captures at normal and narrow widths rather than testing pixel/style constants.

Optional hardware labels must preserve raw diagnostics and PCI identity, reject
ambiguous ID matches, and deserialize older samples without them. Cache local
metadata outside recurring sampling work, bound reads, and identify CPU context
as distinct from a GPU model. Keep unknown sensor states visible and retain
partial-coverage warnings when simplifying diagnostic-heavy views.

Advanced presentation must preserve request payloads, confirmation/conflict gates,
recording note identity and independent inspection. Programmatically opened editors
need explicit persistent IDs across nested menu scopes; cover reveal/persistence
with a headless UI transition check. Missing numeric statistics are availability
states, not zero. Keep destructive actions deliberate and heavy recording tools
on demand; visual refinement cannot add periodic work or discard hidden drafts.

Test harnesses that mutate local services must enforce isolation themselves:
require an empty fixture bus and verify the owned peer PID before mutations.
Temporary XDG paths and a mocked service manager do not isolate session D-Bus.
Add a refusal regression before staging or recording creation; cleanup may touch
only exact fixture identities. Updater fixtures self-spawn an isolated bus when
invoked directly, so a missing command prefix cannot target the live desktop.

Runtime navigation is Applications, GPU Activity and Launch Settings only. Retired
view IDs must normalize to Applications before publishing control/polling state;
validation-only widget access cannot be restored through runtime environment
variables. Maintain the release native navigation check when changing fixture guards.


All launcher publication paths, including explicit saved-profile shortcuts, must
use the shared atomic no-clobber primitive. Never use raw write/truncate on a
launcher destination. Keep disk work off the UI thread. Dashboard work admission
must be bounded and nonblocking, restore rejected pending state, and expose busy
versus stopped errors without logging payloads. Lifecycle control stays independent.

Mutating test fixtures must call the shared `tests/fixture_bus.py` admission guard
before staging/launch and verify the spawned collector's bus-owner PID before
mutations. Temporary XDG roots alone are insufficient. Use an occupied private bus
for refusal regressions; never use an installed service as a negative-test target.

The offline check target and CI compile all Python fixture/packaging modules for
syntax validity as well as checking Rust formatting, Clippy and documentation.
Bus guards do not substitute for real interprocess regression coverage.
