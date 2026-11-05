# Setup, installation and removal

Sidecar targets CachyOS/Arch on Plasma Wayland with AMD integrated and discrete
GPUs. Install from a reviewed checkout; public package availability is separate
from the source repository. The technical package/binary name is `igpu-man`.

## Prerequisites

Rust/Cargo 1.89+, Python 3.11+, `make`, `pkgconf`, coreutils (`sha256sum`), `patch`,
and native Wayland/X11/OpenGL/font libraries are needed to build/check sources.
On CachyOS/Arch, install the build and portable-test prerequisites using pacman:

```sh
sudo pacman -S --needed base-devel rust cargo pkgconf python patch coreutils \
  wayland libxkbcommon libxkbcommon-x11 libglvnd libx11 libxcb fontconfig \
  dbus desktop-file-utils
```

Plasma supplies the desktop/tray environment. `qt6-tools` (`qdbus6`) and `kscreen`
provide optional compositor/display diagnostics; `pciutils` improves model names.
MangoHud is optional (`lib32-mangohud` for 32-bit games); it is not required for
routing or monitoring. Icon regeneration additionally needs `librsvg`; ordinary
builds use checked-in assets. See [contributing](../CONTRIBUTING.md) for optional
native/GPU test tools.

## Try the checkout

```sh
make build
# Terminal 1, inside the desktop session:
./target/release/igpu-mand
# Terminal 2, in the same checkout and session:
./target/release/igpu-man status
./target/release/igpu-man-desktop
```

Do not start a second collector over an installed one. The source-only trial uses
your regular XDG data unless you explicitly isolate it. Monitoring does not tune
GPU settings; confirmed routing/profile/recording actions persist. For disposable developer
tests, use the private-bus fixtures in [contributing](../CONTRIBUTING.md).

## First per-user installation

This installs without administrator access and does not depend on retaining the
checkout. Do this before creating routes that should survive moving the checkout:

```sh
make build
make PREFIX="$HOME/.local" USER_UNIT_DIR="$HOME/.local/share/systemd/user" install
systemctl --user daemon-reload
systemctl --user start igpu-man.service
"$HOME/.local/bin/igpu-man-desktop"
```

Add `~/.local/bin` to your usual PATH if needed for the short command names. The
installed desktop launcher and service use absolute paths. If migrating an old
checkout-specific unit/launcher, back it up and follow
[removing a checkout dependency](troubleshooting.md#removing-a-checkout-dependency).
A higher-priority `~/.config/systemd/user/igpu-man.service` may otherwise keep
starting the old binary. Do not restart an active recording.

For subsequent updates to this active user installation, use `make update-user`;
do not copy binaries over a running installation. [Update/recovery details](updates-and-releases.md)
cover visible-window/recording guards, backups, rollback, leases and prefix changes.

## Optional Sidecar startup

These options start **Sidecar itself**. They do not start routed applications or
change their session-restore settings. Routing normal launches is persistent
regardless of whether the dashboard opens at login.

Enable collector startup if desired:

```sh
systemctl --user enable igpu-man.service
```

To start the per-user installation quietly in the tray:

```sh
mkdir -p "${XDG_CONFIG_HOME:-$HOME/.config}/autostart"
cp "$HOME/.local/share/doc/igpu-man/igpu-man-autostart.desktop" \
  "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/igpu-man.desktop"
"$HOME/.local/bin/igpu-man-desktop" --tray
```

For a `/usr` installation, copy the template from
`/usr/share/doc/igpu-man/igpu-man-autostart.desktop` instead. Review/back up any
existing Sidecar autostart entry before replacing it. Duplicate `--tray` invocations
leave an existing instance unchanged. Cold startup creates no native window or GL
context; if no tray host appears within 15 seconds, a fallback window opens.

Disable the tray entry in Plasma Autostart or remove only that copied entry.
Disable collector login startup separately with `systemctl --user disable
igpu-man.service`; disabling it does not stop the current process.

## Pacman or system-wide installation

For a local pacman package:

```sh
make dist
cd dist
# Inspect PKGBUILD and .SRCINFO first.
makepkg -si
```

Run `makepkg` as your ordinary user. The generated recipe has a pinned checksum and
an upstream release URL; the matching archive in `dist` supplies the local build.
The checked-in `packaging/PKGBUILD` is a template, not a ready AUR submission.
Source publication alone does not create a release asset or distro package.

A manual system installation is also possible after `make build`, using
`sudo make install`. Prefer pacman ownership for a managed system package; use
package updates for pacman-owned files. Sidecar's developer updater never replaces
`/usr` files. Start the user collector with the same `systemctl --user` commands
above; neither package nor manual installation enables login startup automatically.

## Data and service behavior

| Data | Default location |
| --- | --- |
| Profiles, aliases and collection settings | `~/.config/igpu-man/config.toml` |
| Routing originals and recovery journal | `~/.local/share/igpu-man/normal-launches.json` |
| Selected recordings | `~/.local/share/igpu-man/recordings.sqlite` |
| Private logs and updater backups | `~/.local/state/igpu-man/` |
| Derived shortcuts | `~/.local/share/applications/` |

These follow `XDG_CONFIG_HOME`, `XDG_DATA_HOME` and `XDG_STATE_HOME`. The service
inherits the systemd user manager's environment, not every interactive shell's
settings. Custom XDG paths may require an explicit user-service override. Preserve
the routing journal until all routes have been undone. Keep CLI, dashboard and
collector versions together. Config changes currently require a collector restart;
finish recording and review running trials first.

Defaults in [the example configuration](../data/config.example.toml) are 1000 ms
normal sampling, 250 ms recording, a 1024 MiB live-page database budget, a 33.3 ms
frame threshold, and notifications off. SQLite WAL/free pages and logs/backups add
to disk use beyond that budget. Capacity stops recording visibly; existing runs
are never silently deleted. Delete selected recordings explicitly with
`igpu-man record delete ID`. Ordinary telemetry remains in bounded RAM history;
private diagnostic logs persist separately.

The service runs as your ordinary user with `NoNewPrivileges=yes`, `UMask=0077`,
and private data files. `PrivateTmp`, `ProtectSystem` and `PrivateUsers` are
disabled specifically for Sidecar's approved cross-process accounting policy.
See [security boundaries](security.md#user-service-procfs-compatibility) for the
tradeoff; do not add root/ptrace privileges to work around missing accounting.

## Uninstall

1. Use **Applications → Undo** for each routed app, or
   `igpu-man profile undo-normal PROFILE`. Confirm
   `igpu-man profile check-normal` returns an empty list. Resolve conflicts before
   removing the wrapper that managed shortcuts invoke.
2. Quit the dashboard/tray, remove or disable Sidecar's own Plasma autostart entry,
   and run `systemctl --user disable --now igpu-man.service`.
3. Remove installed assets with the method matching your installation:

```sh
# Pacman package:
sudo pacman -R igpu-man
# OR per-user manual installation, from source:
make PREFIX="$HOME/.local" USER_UNIT_DIR="$HOME/.local/share/systemd/user" uninstall
# OR system-wide manual installation, from source:
sudo make uninstall
```

Then run `systemctl --user daemon-reload`. Remove an old checkout-specific unit
only if you installed one. Separate `igpu-man-PROFILE.desktop` shortcuts should be
removed from your XDG applications directory once they are no longer needed.
[Source-free per-user removal](troubleshooting.md#source-free-per-user-removal)
lists the exact installed paths if the checkout is gone.

Uninstall keeps configuration, recordings, routing recovery data, logs and backups.
Delete those separately only if you intend to erase them. Temporary managed frame
logs under `$XDG_RUNTIME_DIR/igpu-man` can be removed after their apps exit.
