# Contributing

Start with [the architecture](docs/architecture.md), [engineering standards](docs/engineering-standards.md),
and [the current review](docs/code-review.md). Repository-specific AI instructions
are in [AGENTS.md](AGENTS.md); they apply to human and AI changes alike.

Use the Rust version declared in `Cargo.toml` or newer. Keep `Cargo.lock` checked in.
Install native development dependencies from the README, then run:

Portable checks also need Python 3.11+, `dbus-run-session`/`dbus-send` (the `dbus`
package), and `desktop-file-validate` (`desktop-file-utils`). Placement tests need
an AMD iGPU, `qdbus6` and the native Wayland EGL demo; native lifecycle tests need
a display. CI uses Xvfb/software OpenGL for its separate window checks.
After `make build`, `python3 tests/packaging.py` validates staged install/removal
and the source archive without installing into the real system.

```sh
make check
make test
```

Use private D-Bus sessions and temporary XDG locations for integration tests.
Native window and GPU checks need a real graphics session; run them separately
and label their results. Do not substitute a mocked graph for hardware validation.

Every behavior change should include its relevant regression coverage and update
the documentation matrix in AGENTS.md. Run the documentation checker before
delivery. PR descriptions should state the concrete trigger, resulting behavior,
validation and remaining risks. Explain data/schema changes and compatibility.

Do not include private logs, recordings or support exports in a PR. Fixtures should
be minimal and synthetic. Screenshots may contain hardware identifiers; inspect
them before committing. Never copy real browser history, configuration secrets or
complete process environments into fixtures.

Keep `sha256sum` (coreutils) available for deterministic build identity. Python
3.11+ runs documentation, release-manifest and regression scripts. Native
help/version checks must work without a bus or display; screenshot failure
injection is compiled only with `igpu-man-desktop/screenshot`. The default release
does not include test failure injection. Use `tests/worker_recovery.py` on a
private bus after that optional build to verify visible worker-failure recovery.

Sidecar's canonical icon is `data/igpu-man.svg`. After editing it, regenerate the
checked-in window/tray PNGs with `python3 tests/render_icons.py` (requires
`rsvg-convert` from librsvg), then inspect full-size and 24 px previews. Ordinary
builds do not require the icon generator. Keep the desktop filename/app ID and
technical identifiers stable unless shipping an explicit migration.
