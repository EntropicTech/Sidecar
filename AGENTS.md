# Working on igpu-man

Read `README.md`, `docs/architecture.md`, and `docs/engineering-standards.md`
before changing behavior. Read `docs/code-review.md` for outstanding risks and
`docs/troubleshooting.md` before debugging a live installation.

## Product invariants

- Stay unprivileged and local. Never introduce uploads, root helpers, global GPU
  tuning, automatic application termination, or migration of running workloads.
- GPU identity is PCI; process identity is PID **and start ticks**. An open DRM
  descriptor is not activity. Missing, inaccessible, stale and idle are different.
- Rendering and video decoding are separate observations. Do not infer software
  decoding from absent decoder activity or claim gaming gains without experiments.
- Keep launches explicit, preserve argv without a shell, and preserve original
  launchers. Trials do not become saved profiles until Keep.
- Never log browsing data, RPC payloads, raw application stdout/stderr, complete
  process arguments/environments, profile environment values, or configuration
  excerpts. Use stable events, request IDs and allowlisted fields.

## Change discipline

- Reproduce reported failures before claiming a fix; add a regression check for
  state transitions, parsers, persistence, or identity changes. Avoid tests that
  merely mirror constants or styling.
- Use `Result` and context at recoverable boundaries. Never silently discard
  persistence, worker, shutdown or cleanup failures. Do not turn unknown into zero.
- Keep the impact light: share immutable samples, sleep idle workers, bound queues
  and job concurrency, preserve sampling defaults, and measure release CPU/RSS plus
  live latency when changing collection/storage. No per-sample diagnostic log spam.
- Keep UI control independent of collector RPCs. Use bounded calls and reads.
  Recover from ordinary missing capabilities; fail visibly on corrupted invariants.
- Use atomic, private configuration writes and explicit schema compatibility.
- Keep source changes focused. Do not claim all review findings are resolved when
  follow-ups remain; update their status and acceptance criteria.

## Required documentation updates

| Change | Update in the same change |
| --- | --- |
| CLI, setup, defaults or window lifecycle | `README.md`, relevant troubleshooting |
| Collector or dashboard D-Bus methods/types | `docs/api.md` |
| Ownership, threads, state or persistence | `docs/architecture.md` |
| Logging, errors, limits or recovery | `docs/engineering-standards.md`, `docs/troubleshooting.md` |
| New checks or hardware observations | `docs/validation.md`; identify fixtures and limits |
| Known risk fixed/deferred | `docs/code-review.md` with evidence or acceptance criteria |
| New documentation file | docs index and packaging/install coverage |

## Updating the installed developer copy

After implementation, use `make update-user` for the active user install. It must
keep its safety checks: never bypass active-recording/maintenance guards or update
pacman-owned paths. Do not use ad hoc source-to-bin copies as the normal update
workflow. Update release documentation/changelog with behavior and distribution
changes; never publish/tag without exact-source validation and source review.

## Verification

- Run `make check` and `make test` for Rust/backend behavior. CI must enforce
  formatting, all-feature Clippy, unit/integration tests and documentation checks.
- Tray residency: build `cargo build -p igpu-man-desktop --example tray_fixture`,
  then `dbus-run-session -- python3 tests/tray_lifecycle.py` and
  `dbus-run-session -- python3 tests/tray_startup.py`; verify actual Plasma
  with `--live` only when no dashboard is already running.
- Native lifecycle: `dbus-run-session -- python3 tests/dashboard_lifecycle.py`.
  Real Plasma tray: `python3 tests/dashboard_lifecycle.py --live` only when no
  other dashboard owns the control name; never close unrelated applications.
- GPU placement: `dbus-run-session -- python3 tests/placement_workflow.py`.
- Packaging: `make build`, then `python3 tests/packaging.py`; installation/removal
  must use its temporary DESTDIR, never the real prefix as a test.
  Render UI changes with the screenshot feature and inspect the resulting image.
- Use isolated session buses and temporary XDG config/data/state by default.
  Never stop the real collector or an active recording as test cleanup.
- Report exactly what passed, what was not tested, and measured repeatability
  limits. A CI pass does not establish suspend/resume or gaming performance.

Session instructions and user authorization take precedence over this file.
