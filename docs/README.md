# Documentation

Start with the [project README](../README.md) for the purpose and main workflow.
Sidecar's production dashboard has Applications, GPU Activity and Launch Settings.

## Use and maintain Sidecar

| Task | Guide |
| --- | --- |
| Build, install, enable optional tray startup, or uninstall | [Setup](setup.md) |
| Route an app, verify rendering/decoding, or undo | [Everyday use](user-guide.md) |
| Run commands, custom profiles, or optional recordings | [CLI reference](cli.md) |
| Diagnose missing metrics, Firefox, window or routing problems | [Troubleshooting and AI debugging](troubleshooting.md) |
| Update a user install, recover, or prepare a package | [Updates and releases](updates-and-releases.md) |
| Compare recorded game/playback conditions | [Experiments](experiments.md) |
| Understand privacy and report a security problem | [Public security policy](../SECURITY.md) |

## Develop and review

| Topic | Reference |
| --- | --- |
| Build/check workflow and contribution expectations | [Contributing](../CONTRIBUTING.md) |
| Ownership, state, threads and persistence | [Architecture](architecture.md) |
| Errors, logging, performance and review standards | [Engineering standards](engineering-standards.md) |
| Collector/dashboard D-Bus contract | [API](api.md) |
| Findings, evidence and remaining acceptance criteria | [Code review](code-review.md) |
| Latest acceptance summary and historical measurements | [Validation](validation.md#current-acceptance-summary) |
| Trust boundaries, resource limits and dependency maintenance | [Security](security.md) |
| Patched third-party source, licenses and removal criteria | [Native compatibility provenance](../vendor/README.md) |
| Repository instructions for AI and human changes | [AGENTS.md](../AGENTS.md) |
| User-facing changes by development version | [Changelog](../CHANGELOG.md) |

## Keeping docs current

README owns the project overview and quick path. Setup owns install/removal;
Everyday use owns UI tasks and evidence; CLI/API references own their interfaces.
Architecture describes the current design. Code review tracks unresolved work;
validation is a dated evidence log, so older interface descriptions there are
historical rather than current instructions.

Use the change-to-document matrix in [AGENTS.md](../AGENTS.md#required-documentation-updates).
New guides must be indexed here and included in installation/source archives.
`make check` verifies local links, public CLI/API coverage, config examples,
version consistency and packaging contracts. Packaging tests check every docs asset.
Review commands, defaults and claims against the implementation; keep limitations
and dated hardware evidence explicit. Never paste private support captures into docs.
