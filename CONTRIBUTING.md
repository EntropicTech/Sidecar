# Contributing

Start with the [README](README.md), [architecture](docs/architecture.md),
[engineering standards](docs/engineering-standards.md), and
[current review](docs/code-review.md). [AGENTS.md](AGENTS.md) defines repository
invariants, the documentation update matrix and safe deployment/test practices.

## Build and check

Install the [setup prerequisites](docs/setup.md#prerequisites). Use the Rust
version declared in Cargo.toml or newer, and keep Cargo.lock checked in.

```sh
make check
make test
make build
python3 tests/packaging.py
```

`make check` verifies vendor provenance, Rust formatting, all-target/all-feature
Clippy, local documentation contracts and Python syntax. `make test` builds the
workspace/fixtures and runs Rust plus private-session D-Bus integration tests.
Packaging tests use temporary installation prefixes, not your real installation.

Python 3.11+, `busctl` (systemd), `dbus-run-session`/`dbus-send` (dbus), `patch`,
`sha256sum` (coreutils), and `desktop-file-validate` (desktop-file-utils) are required
for the portable checks. Optional GPU placement checks also need an AMD iGPU,
`qdbus6` and `eglgears_wayland`. Native checks need a display; hosted CI uses
Xvfb/software OpenGL for its separate lifecycle checks.

## Test safely

Use private session buses and temporary XDG config/data/state. Mutating fixtures
require an empty protected bus and verify their owned collector PID. Temporary
XDG paths alone do not isolate D-Bus. Never stop an installed collector or active
recording to make a test pass. Explicit live checks must refuse unrelated windows
and affect only their owned fixture processes.

The exact native, tray, routing and measurement commands are in
[AGENTS.md](AGENTS.md#verification) and [validation](docs/validation.md).
Mark hardware-dependent skips and untested scenarios honestly. A mocked chart or
software renderer cannot validate GPU placement, suspend/resume or gaming gains.

## Keep changes reviewable

- Include regression coverage for changed parsers, identities, state transitions,
  persistence or recovery. Keep fixtures minimal and synthetic.
- Update task-oriented user instructions and relevant technical docs in the same
  change. Use the [documentation ownership map](docs/README.md#keeping-docs-current)
  and run `make check`; link/coverage checks do not replace a semantic review.
- State the concrete trigger, resulting behavior, validation and remaining risks
  in PRs. Explain data/schema compatibility and any package/runtime changes.
- Exclude private logs, configuration, recordings and exports from commits/issues.
  Inspect screenshots; use synthetic app names/data for public UI captures.
- Read [security boundaries](docs/security.md) before changing dependencies or
  input/workflow boundaries. Run `make security-check` with the reviewed audit tool
  before releases. Keep workflow actions SHA-pinned and maintenance notices visible.

Default releases exclude the `igpu-man-desktop/screenshot` feature. It enables
controlled captures and failure injection only for validation; the production
navigation remains three tabs. After a screenshot build, rebuild the ordinary
release before packaging. The bundled eframe patch must continue passing
`python3 tests/verify_vendor.py`; see [provenance/removal criteria](vendor/README.md).

## Assets and deployment

The canonical icon is `data/igpu-man.svg`. Regenerate window/tray PNGs with
`python3 tests/render_icons.py` (requires `rsvg-convert` from librsvg), then inspect
full-size and 24 px previews. Ordinary builds use the checked-in assets. Preserve
desktop filename/app ID and technical identifiers unless shipping a migration.

Use `make update-user` for an active developer installation; preserve its guards.
See [updates and releases](docs/updates-and-releases.md) for exact-source checks,
recovery and the separate AUR/CachyOS submission path. Pushing a commit is not a
release, and configured CI is not evidence that its hosted run passed.
