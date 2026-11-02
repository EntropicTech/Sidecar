# Updates and releases

Upstream: https://github.com/EntropicTech/Sidecar. The display name is Sidecar;
binaries, configuration and package identity currently remain `igpu-man`.

## Updating this developer installation

Save editor changes and close the dashboard to its tray, then run:

```sh
make update-user
```

This runs checks/tests, builds a release, stages a prefix-aware installation and
verifies artifact hashes before touching installed files. It updates the active
user-owned install under `~/.local`; `USER_PREFIX` can select another home prefix
with its matching user unit. It refuses root/system installs and never writes
pacman-owned `/usr` files. It requires the installed collector to advertise
`coordinated_updates`; older development installs need a one-time deliberate
bootstrap rather than an unsafe automatic fallback.

An active recording blocks the update. A five-minute nonce lease prevents new
mutations while preparing/restarting, and a temporary user-runtime systemd drop-in
carries the lease into the replacement collector. Sampling continues until the
planned short restart. The updater saves launch specifications (never scraped
argv/environments), snapshots the stopped database with SQLite backup, and backs
up config and installed files privately. Files are replaced atomically; the
service and prior tray state are restored, live PID/start identities re-registered,
and profiles/recordings checked. Trial evidence must be gathered fresh after a
collector restart. Unsubmitted GUI drafts are temporary across Quit; save them
before running an update.

The updater never overwrites/deletes profiles, recordings, autostart entries or
service enablement. A failed restart restores the previous programs. User data
is not blindly rolled back: schema incompatibility requires explicit review and
the retained consistent backups. This is not a filesystem-wide ACID transaction.

Private state: `$XDG_STATE_HOME/igpu-man/updates` (default
`~/.local/state/igpu-man/updates`), mode 0700, contents 0600. Only the last two
successful backup sets are retained. Incomplete transactions are never pruned and
block another deployment. Review the journal and restore the old programs with:

```sh
make recover-user RUN=/absolute/path/to/run-transaction
```

Recovery refuses an active recording and leaves saved user data untouched. Keep
free space for staged binaries plus a database/program backup. A hidden, bounded
stdin-to-D-Bus CLI bridge keeps registration payloads/nonces out of process arguments; recovery builds that bridge before use. No updater runs
in the background; there is no automatic download, poller or privilege helper.

## Packaging and release bar

`make dist` creates a deterministic source archive (normalized ownership,
permissions and timestamps), SHA256SUMS, a checksum-pinned PKGBUILD and, when Arch makepkg is available, `.SRCINFO`.
Non-Arch source CI can generate archives without makepkg; submission/release CI
must generate and validate `.SRCINFO` using the official tool.
`SOURCE_DATE_EPOCH` optionally sets the normalized timestamp; default is zero.
The checked-in `packaging/PKGBUILD` is a template and cannot itself be submitted.
Submit only generated metadata that matches the published source archive. Do not
use `SKIP` checksums for a fixed release, add binaries to AUR, or reference a local
checkout in a public recipe.

Arch GCC object LTO is disabled for the bundled SQLite C objects; Cargo keeps
its own thin Rust LTO. Rust dependencies are fetched in prepare with `--locked`; build/check use
`--frozen` and a controlled target directory. Production does not enable the
screenshot/failure-injection feature. The Arch CI job builds a real package as a
non-root user in a fresh Arch container and fails on namcap errors.
The icon-theme hierarchy and direct libgcc dependency are declared. Warnings for
runtime-loaded EGL/Wayland/X11/font libraries and D-Bus/systemd remain reviewed
requirements even when ELF dependency scanning labels them unused; the ELF
interpreter can also be reported as unused. This is useful isolation,
not a claim that devtools clean-chroot acceptance has already passed. Before
submission run the current Arch clean-chroot procedure and resolve namcap findings,
validate runtime dependencies and installation/removal, and repeat native lifecycle,
update/rollback and overhead checks. Keep unresolved hardware acceptance limits
visible in validation.md. CI configuration is not evidence of a successful hosted run.

A release must have passing CI for its exact source revision, matching version and
changelog, reviewed source contents/checksums, no private data, and a stable source
URL. Create a tested `vVERSION` tag; the manual prepare-release workflow creates
only a draft with the source archive and Arch metadata. Review/publish that draft
before anyone uses the public AUR recipe. No automated AUR push or distro request
is configured. Keep maintainer identity, package name availability and AUR SSH
access verified before first submission.

## Distribution route

The AUR accepts maintained PKGBUILD/.SRCINFO repositories; it is separate from a
CachyOS binary repository. Start with a reviewed AUR submission, then request
CachyOS inclusion. Acceptance is a maintainer decision, not an upload entitlement.
CachyOS documents GitHub/forum request channels. No AUR package or CachyOS inclusion
is claimed merely because GitHub source/CI exists.

Primary references:

- [CachyOS package requests](https://wiki.cachyos.org/cachyos_basic/faq/#submitting-package-requests-to-cachyos)
- [Arch Rust package guidelines](https://wiki.archlinux.org/title/Rust_package_guidelines)
- [AUR submission guidelines](https://wiki.archlinux.org/title/AUR_submission_guidelines)
- [SRCINFO manual](https://man.archlinux.org/man/SRCINFO.5.en)
- [Arch clean chroot](https://wiki.archlinux.org/title/Building_in_a_clean_chroot)
