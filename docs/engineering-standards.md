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
