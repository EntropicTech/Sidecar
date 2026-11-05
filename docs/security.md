# Security boundaries and maintenance

Sidecar runs as the desktop user on the local session bus. There is no remote
listener or privileged helper. Session-bus access is an account-level trust
boundary, not authorization between mutually hostile applications. Do not expose
the collector wholesale to sandboxed apps. A compromised account can already
modify its own profiles and programs; these controls reduce accidental disclosure
and bound Sidecar's resource use, not contain arbitrary same-user malware.

## Profile transport and launcher recovery

Dashboard trial profiles travel over an anonymous stdin pipe to the hidden CLI
wrapper. The wrapper requires EOF and rejects input above 1 MiB before parsing.
There is no legacy profile-JSON command-line argument. Arguments, explicit
profile environment assignments and working directory retain their launch
semantics. The launched application's own argv/environment remain visible under
normal OS access rules; do not treat profiles as secret storage.

`pending_launcher` is private durable Keep/recovery state. Public `save_profile`,
`register_launch` and trial inputs reject non-null intents. Normal saved-profile
launches omit recovery state from registration. Keep alone constructs the desktop
entry; startup reconciles locally persisted intents, preserving conflicts and
symlinks. Existing v1 config recovery remains readable. Private config-file access
is trusted; editing one's own configuration is not a separate security boundary.

## Resource limits

Admission happens before JSON parsing and live-state cloning: four live requests
(including capabilities), two recording reads and eight mutations. Parsing runs on
blocking workers; live/read permits stay with blocking work if the caller cancels.
There is no unbounded waiting queue for API admission. The ordered writer retains
its separate bounded FIFO for controls and selected samples. Busy replies are
D-Bus `LimitsExceeded`; retry reads with backoff, inspect state before retrying
mutations. These are concurrency limits, not per-peer fairness or a guarantee
against every possible local denial of service. The bus also buffers messages.

Diagnostic helpers capture at most 64 KiB of UTF-8 through a nonblocking pipe,
with a two-second deadline. Oversized, invalid, unsuccessful or timed-out output
is unavailable rather than a silently truncated success. Capture creates no
output file and reaps its direct helper on failure. Diagnostic tools are resolved
using the user's PATH; they are trusted local programs. Raw output is never logged.

## Dependency and CI policy

`make security-check` runs cargo-audit against Cargo.lock. Install the reviewed tool
with `cargo install cargo-audit --version 0.22.2 --locked`. It fetches public RustSec
advisories and crate-index metadata; ordinary offline `make check`/`make test` do
not require it. The `security` workflow runs on pushes, pull requests, manual
requests and weekly schedules. Vulnerabilities, unsound notices and yanked crates
fail the gate. Unmaintained notices remain visible warnings, without advisory
ignore entries. A passing scan covers known crate advisories, not all application,
bundled native-library, system-library, driver or undiscovered vulnerabilities.

The November 4, 2026 recheck retained the five maintenance notices from the
November 1 scan, with no matched vulnerability advisories:

| Crate | Notice | Follow-up |
| --- | --- | --- |
| paste | [RUSTSEC-2024-0436](https://rustsec.org/advisories/RUSTSEC-2024-0436.html) | Replace via the upstream dependency using it. |
| ttf-parser | [RUSTSEC-2026-0192](https://rustsec.org/advisories/RUSTSEC-2026-0192.html) | Follow the egui/font dependency migration and validate native rendering. |
| zvariant | [RUSTSEC-2026-0370](https://rustsec.org/advisories/RUSTSEC-2026-0370.html) | Coordinate migration to zbus 6 with the tray dependency. |
| zbus_names | [RUSTSEC-2026-0371](https://rustsec.org/advisories/RUSTSEC-2026-0371.html) | Same D-Bus migration. |
| zvariant_derive | [RUSTSEC-2026-0372](https://rustsec.org/advisories/RUSTSEC-2026-0372.html) | Same D-Bus migration. |

These migrations remain follow-ups, not hidden exceptions or patched
vulnerabilities. Recheck before releases and review each Dependabot proposal;
there is no automatic merge. External workflow actions use full commit SHAs with
readable version comments and checkout credentials are not persisted. Dependabot
checks Cargo and GitHub Actions weekly. Keep action pins immutable when upgrading;
see [GitHub's guidance](https://docs.github.com/en/actions/reference/security/secure-use).
The audit tool is version-pinned; reviewed version bumps remain manual.

Local update-manifest hashes detect corrupt/mixed artifacts; they are not publisher
signatures. Developer updates use explicitly built local artifacts, not downloaded
remote updates.

## User-service procfs compatibility

The user service explicitly sets `PrivateTmp=no`, `ProtectSystem=no` and
`PrivateUsers=no`. User-service mount sandboxing can create a user namespace and
prevent same-account `/proc/PID/exe` and `fdinfo` inspection even when the direct
PrivateUsers setting is off. The compatibility policy removes those namespace
boundaries so the collector can attribute GPU work to other user processes.

This was an explicitly approved Sidecar-only policy change. The collector shares
the user's `/tmp` and ordinary filesystem view; it loses the private temporary
namespace and read-only system-mount sandbox. `NoNewPrivileges=yes`, `UMask=0077`,
private data files and ordinary user ownership remain. There is no root helper,
extra ptrace capability, global sysctl change or change to other services.

A custom unit override enabling mount/user namespaces may reduce accounting again.
Validate the effective installed unit and real cross-process coverage rather than
assuming a standalone run proves service compatibility. See Q22 in the
[review](code-review.md) and the [runbook](troubleshooting.md).

## Regression checks

`make test` includes `dbus-run-session -- python3 tests/security_boundaries.py`.
It refuses an existing collector, uses private XDG data, and checks only owned
process command lines with synthetic values. It verifies rejected recovery inputs
cannot publish a launcher after restart, malformed/oversized stdin, direct argv
and environment preservation (the latter needs an AMD GPU). Rust tests cover
request saturation, parse failures, cancellation, valid recovery/conflicts, and
bounded/noisy/hung helpers. Real browser, game and suspend/resume validation remain
separate hardware checks. See [validation](validation.md) and [reporting](../SECURITY.md).


## Reviewed native compatibility source

The local eframe patch retains its upstream version with explicit provenance and
a one-file Linux retirement change. `make check`/`make build` verify the unchanged
registry files and exact reversible patch before compilation; dependency auditing
continues separately. Source identity includes vendored Rust/manifests. This is
a compatibility fix, not an upstream security certification. License notices and
provenance ship in the source/installed docs. See [maintenance](../vendor/README.md).

## Opt-in desktop integration

Normal-launch APIs accept a saved profile, installed desktop ID, installed CLI
path and option flags, never caller-authored undo content or artifact paths. Setup
checks executable identity, blocks profiles managed elsewhere and recording
profiles, and requires preview freshness at Apply. Final and ancestor symlinks,
special files, oversized files and unsupported INI expansion/immutable forms are
rejected. New paths remain in the user's XDG application/MIME files; only legacy retirement/Undo touches previously owned autostart/session files;
no system launcher, global environment or application D-Bus service is replaced.

Original local artifact bytes/handler keys are kept in an 8 MiB schema-2 private
journal (at most 128 routes / 40 changes each); normal file reads cap at 128 KiB,
startup scans at 1024 entries per root / 24 matches. The journal is trusted private
recovery state and can contain original command values; do not publish it. Source
fingerprints/preview tokens detect changes, not hostile same-account tampering.
No filesystem-wide atomicity or protection against racing same-user editors is
claimed. Conflicts retain originals; pending operations require explicit resume.
Undo should precede uninstallation. Managed Firefox forwarding requires live
PID/start identities matching its registered profile/GPU; this is requested
placement evidence, not proof of renderer/decoder activity. Fixed local failure
notifications contain no arguments, URLs or raw errors.

A managed explicit launch may start only `igpu-man.service` through the existing
local systemd user manager when the collector is missing. It never enables units,
replaces running services or connects to another session bus as a fallback.


The primary app API accepts only an installed desktop ID, CLI path and freshness
token. It constructs settings server-side and never starts apps at login. Its
private prepared-profile journal is validated against name/fingerprint/ownership
before recovery; it is not a public input. Reads support v1/v2, writes v2, with
incompatible old builds failing visibly. UI Undo can remove only unchanged settings
created by quick setup. No existing profiles or recordings are deleted as part of
this workflow. Legacy login retirement is journaled, conflict-checked and preserves
normal routing. No new dependencies or uploads are introduced.


GPU presentation metadata reads only the fixed local libdrm database and cpuinfo,
with 512 KiB/1 MiB bounds, plus short product/device/revision sysfs metadata on
first device observation. Names are presentation data, never launch selectors or
commands. Invalid/ambiguous optional metadata falls back to existing labels.
No remote lookup, new dependency, process-environment collection or raw-data log
is introduced. GPU routing continues to use PCI identity.


## Integration-test isolation

Updater fixtures self-create a private session bus by default, reject existing
collector/dashboard/user-manager owners before creating data or making mutations,
and verify that the collector bus peer's PID is their owned child. Their nested
occupied-bus regression must refuse without changing that collector's catalog.
Temporary XDG directories alone do not isolate D-Bus; fake service managers must
never use an arbitrary existing bus owner as proof their child is ready. This
protects against accidental harness invocation on a live desktop, not hostile
same-user manipulation of the test's internal environment marker.


Saved-profile launcher publication is serialized through the collector writer.
Existing destinations, including dangling symlinks, are preserved with atomic
no-clobber publication. Dashboard explicit work has eight waiting slots and rejects
excess work without blocking native lifecycle. Synthetic mutating fixtures require
an empty bus and verify owned collector PIDs before any mutations; test refusal
only on isolated buses, including while a synthetic recording is active.
