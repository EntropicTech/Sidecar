# Security and privacy

Sidecar is an unprivileged local desktop utility. It never requires a root helper
or uploads telemetry. Profiles are explicit launches; monitoring does not alter
GPU clocks, power settings, compositor routing or existing application instances.

Do not put browsing data, process environments, profile environment values,
private recordings or unredacted logs in public issues. Use the diagnostic runbook
to produce a small, reviewed reproduction. A GPU device descriptor is not proof
of measured rendering/decoding, and missing accounting is not zero activity.

Report a suspected security issue privately using GitHub's private vulnerability
reporting if enabled for https://github.com/EntropicTech/Sidecar. If it is not
available, contact the maintainer through the public GitHub profile to arrange a
private channel without publishing sensitive details. No dedicated response-time
commitment or supported stable release series exists yet.

Developer updates apply only to user-owned installations. Pacman owns system
packages; Sidecar never self-updates `/usr`. Saved data is outside installed files.
Rollback restores programs, not user data; private consistent data backups are
retained for explicit schema recovery. See the update/release documentation.
