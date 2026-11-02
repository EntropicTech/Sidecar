# Senior engineering review

Review scope: workspace architecture, telemetry/accounting, launch/profile safety,
parsing/persistence, D-Bus/thread failures, native lifecycle, logging, packaging,
tests, documentation and AI maintenance. This is an early local utility, not a
production-readiness certification. Hardware claims require separate experiments.

## Findings and disposition

| ID | Severity | Finding | Disposition / evidence |
| --- | --- | --- | --- |
| Q01 | High | Wayland window-close canceled closing, requested unsupported hide, then stopped rendering | Fixed: Close destroys the Wayland window/renderer and parks the model/tray; Show recreates it. Missing-tray exit, stopped parked redraws and watcher-loss recovery are covered. Quit exits desktop/tray; collector remains. |
| Q02 | High | Relaunch could create duplicate dashboards or lose a reopen to an exiting owner | Fixed: exclusive ownership, no singleton ownership by probes, immediate shutdown intent and owner-loss retry. Rapid reopen reproduced the lost-Show race; unit/native regressions cover it. Close/Show now preserves desired-open intent across native-close echoes; only Quit seals resident shutdown. |
| Q03 | High | GUI control queued behind collector IPC | Fixed: separate lifecycle channel/connection; paused-collector quit test verifies responsiveness. |
| Q04 | High | Naive CSV header detection could reach `unwrap` on a quoted combined column | Fixed: validated named-column lookup returns errors; malformed-header regression fixture. |
| Q05 | High | Predictable config temporary file, non-private permissions and no file sync | Fixed: private exclusive temporary file, sync and atomic rename; symlink/permission regression tests. |
| Q06 | High | Worker JoinErrors and recording cleanup failures could be silently discarded | Hardened: supervised daemon workers, explicit storage/cleanup logging, identity-guarded Stop. |
| Q07 | High | Sparse diagnostics and inconsistent ad-hoc logging made failures hard to correlate | Hardened: private bounded JSONL logs, request IDs, panic location/backtrace, logs CLI, doctor metadata and runbook. |
| Q08 | Medium | Unkept launch setup could be lost when the dashboard exits | Hardened: collector-owned pending trials can resume with fresh identity-bound verification; no auto-Keep. Unlaunched editor drafts survive tray parking but remain temporary across Quit. Collector restart invalidates live evidence. |
| Q09 | Medium | Large reads/imports/exports share the sampler state lock | Fixed: immutable sampler snapshots, bounded sleeping writer, independent WAL readers and UI recording worker. Large-recording/native responsiveness and FIFO Stop tests pass; finite queue exhaustion is visible. |
| Q10 | Medium | Config + generated launcher are not a crash-atomic multi-file transaction | Hardened: committed pending-launcher intent, atomic no-clobber publication and startup reconciliation. Fault-point/conflict/symlink fixtures preserve originals. Committed profiles remain usable with publication warnings; no global ACID claim. |
| Q11 | Medium | Monolithic desktop source and broad string/JSON dispatch make changes harder to audit | Hardened: extracted live state, dispatch, writer, dashboard view-model/recording worker/health modules; shared typed operation names/classification preserve v1 wire compatibility. View widgets and JSON payload boundaries still have room for incremental decomposition. |
| Q12 | Medium | Full-database detail/export reads can allocate large vectors | Hardened: streaming detail/export, bounded legacy arrays, stopped-recording cursor pages, GPU-only timeline and cached peak frame series. Read concurrency/deadlines and explicit size limits apply; full statistics/exports retained. |
| Q13 | Medium | GUI worker lock poisoning remains fail-stop rather than recoverable | Hardened: supervised worker states, visible recovery view, poison-race frame abandonment without reading poisoned data. Native failure/poison/clean-close/fresh-reopen tests pass; collector remains independent. |
| Q14 | Medium | Artifacts lack a stable source/build identifier beyond version and PID | Fixed: deterministic Rust-source/dependency digest plus compiler/target metadata; structured logs, version/status/capabilities, doctor executable hash and release/install artifact manifests. Exact-binary reproducibility across toolchains is not claimed. |
| Q15 | Medium | Installation/CI/documentation checks lagged behind newly added commands and docs | Hardened: documentation contracts, link/API/config checks, contributor/agent instructions, and package/install coverage. |
| Q21 | Medium | A matching saved executable can be mistaken for an applied profile or select the wrong target among several profiles | Fixed: launch-path projection uses identity-bound registrations, labels unregistered/unknown, preserves alternatives, gives actual registered intent priority and suppresses excluded-workflow suggestions. Unit/collector fixtures cover ambiguous targets, wrapper exit and PID reuse; native audit capture inspected. |
| Q20 | Medium | Static version probes run every 30 seconds and parked telemetry wakes on a timer | Hardened: diagnostics-worker-owned five-minute cache invalidated by executable/package metadata, separate display refresh, event-driven parked receive with explicit native visibility wake-ups. Cache expiry/missing-tool/change tests and cold/warm tray wake-counter regressions cover behavior; hardware discovery remains live. |
| Q19 | High | Replacing a live developer install can race recordings, lose registrations or strand a stopped service | Hardened: recording exclusion and restart-carried nonce lease, stopped SQLite backup, private file transaction/rollback, explicit stopped-service recovery, identity-bound re-registration and data/catalog checks. Real collector fixtures cover refusal, success, restart failure and interrupted recovery; actual per-user updates preserve autostart and saved data. |
| Q16 | High | Failed recording fetch could leave the previous recording's timeline displayed | Fixed: clear failed sample/frame results; desktop regression verifies prior frame data is not reused. |
| Q17 | Medium | Serde ignores unknown configuration fields, so typos or newer settings may be silently lost on a later save | Fixed: strict unknown-key config/profile/specification policy with safe error locations, known omitted-field v1 defaults, typo/compatibility/non-rewrite fixtures. Future unknown settings require a compatible build. |
| Q18 | Low | Native dashboard/daemon argument handling is manual and may ignore unknown flags | Fixed: typed native parsers; display/bus-free help/version/error tests verify no service/window/database is created. |

## Good foundations to preserve

PCI/PID-start identities, metric provenance and availability, DRM deduplication,
explicit argv-preserving launches, separate render/decode observations, bounded RAM
history, optional game logging, local-only operation and repeatable recordings are
the right foundations. Keep their uncertainty visible as the product grows.

## Release bar

No unresolved high-severity correctness/data-loss findings; documented remaining
limitations; green portable checks; native lifecycle/tray checks; validated staged
install/uninstall; no secret-bearing fixtures or logs. Recheck GPU placement,
disconnect/reconnect, suspend/resume and overhead on the intended hardware. A UI
smoke test or an idle counter is not a game-performance result.

The authoritative validation evidence is in [validation.md](validation.md). Keep
this table current: mark fixes only after regression evidence, and attach explicit
acceptance criteria to deferred work. Do not remove findings simply because a
warning has been hidden.
