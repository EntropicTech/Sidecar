//! Opt-in desktop routing. Durable private undo journal; no sampler work or shell.
use crate::{
    config::{Config, Paths},
    launch, launchers,
    model::Profile,
};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    hash::{Hash, Hasher},
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

const FILE_LIMIT: usize = 128 * 1024;
const JOURNAL_LIMIT: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Setup {
    pub profile: String,
    pub desktop_id: String,
    pub cli_path: PathBuf,
    #[serde(default)]
    pub login: bool,
    pub default_browser: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Apply {
    pub setup: Setup,
    pub token: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preview {
    pub setup: Setup,
    pub token: String,
    pub changes: Vec<String>,
    pub warnings: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Status {
    pub profile: String,
    pub desktop_id: String,
    pub phase: String,
    pub launcher_ok: bool,
    pub login_ok: Option<bool>,
    pub browser_defaults_ok: Option<bool>,
    pub restore_excluded: Option<bool>,
    pub conflicts: Vec<String>,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema_version: u32,
    entries: BTreeMap<String, Route>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Route {
    setup: Setup,
    phase: String,
    executable: String,
    profile_fingerprint: String,
    source_fingerprint: String,
    source_path: Option<PathBuf>,
    changes: Vec<Change>,
    warnings: Vec<String>,
    #[serde(default)]
    created_profile: bool,
    #[serde(default)]
    prepared_profile: Option<Profile>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
enum Target {
    Launcher(String),
    Autostart(String),
    Mime(String),
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
enum Change {
    Replace {
        target: Target,
        before: Option<String>,
        after: String,
    },
    Exclude {
        before: Option<String>,
        added: Vec<String>,
        required: Vec<String>,
    },
}
impl Target {
    fn path(&self, paths: &Paths) -> Result<PathBuf> {
        Ok(match self {
            Self::Launcher(id) => {
                valid_id(id)?;
                paths.applications.join(id)
            }
            Self::Autostart(id) => {
                valid_id(id)?;
                config_root(paths)?.join("autostart").join(id)
            }
            Self::Mime(key) => {
                ensure!(
                    BROWSER_TYPES.contains(&key.as_str()),
                    "Unsupported association key"
                );
                config_root(paths)?.join("mimeapps.list")
            }
        })
    }
    fn label(&self) -> String {
        match self {
            Self::Launcher(_) => "Normal application launcher".into(),
            Self::Autostart(id) => format!("Login entry: {id}"),
            Self::Mime(key) => format!("Default handler: {key}"),
        }
    }
}
const BROWSER_TYPES: &[&str] = &[
    "x-scheme-handler/http",
    "x-scheme-handler/https",
    "text/html",
    "application/xhtml+xml",
];
fn config_root(paths: &Paths) -> Result<&Path> {
    paths
        .config
        .parent()
        .and_then(Path::parent)
        .context("Missing XDG configuration root")
}
fn session_path(paths: &Paths) -> Result<PathBuf> {
    Ok(config_root(paths)?.join("ksmserverrc"))
}
fn journal_path(paths: &Paths) -> PathBuf {
    paths.data.join("normal-launches.json")
}
fn valid_id(id: &str) -> Result<()> {
    ensure!(
        id.len() <= 180
            && id.ends_with(".desktop")
            && !id.starts_with("igpu-man")
            && !id.contains("..")
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)),
        "Select a supported desktop application ID"
    );
    Ok(())
}
fn check_parents(path: &Path) -> Result<()> {
    for parent in path.ancestors().skip(1) {
        match fs::symlink_metadata(parent) {
            Ok(m) => ensure!(
                m.is_dir() && !m.file_type().is_symlink(),
                "A managed destination has a symlink or non-directory parent"
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).context("Cannot inspect managed directory"),
        }
    }
    Ok(())
}
fn read(path: &Path, limit: usize) -> Result<Option<String>> {
    check_parents(path)?;
    let file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).context("Cannot read managed file safely"),
    };
    ensure!(
        file.metadata()?.is_file(),
        "Managed destination is not a regular file"
    );
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= limit, "Managed file exceeds its size limit");
    Ok(Some(
        String::from_utf8(bytes).context("Managed file is not UTF-8")?,
    ))
}
fn write(path: &Path, content: Option<&str>) -> Result<()> {
    check_parents(path)?;
    read(path, JOURNAL_LIMIT)?; // Reject symlinks/special files even when removing.
    let parent = path.parent().context("Managed path has no parent")?;
    fs::create_dir_all(parent)?;
    if let Some(content) = content {
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
        temporary.write_all(content.as_bytes())?;
        temporary.as_file().sync_all()?;
        temporary.persist(path).map_err(|e| e.error)?;
    } else if path.exists() {
        fs::remove_file(path)?;
    }
    fs::File::open(parent)?
        .sync_all()
        .context("Managed directory sync failed; inspect setup status before retrying")?;
    Ok(())
}
fn load(paths: &Paths) -> Result<Journal> {
    let Some(raw) = read(&journal_path(paths), JOURNAL_LIMIT)? else {
        return Ok(Journal {
            schema_version: 1,
            entries: BTreeMap::new(),
        });
    };
    let j: Journal = serde_json::from_str(&raw)
        .context("Invalid normal-launch undo journal; originals were not changed")?;
    ensure!(
        [1, 2].contains(&j.schema_version) && j.entries.len() <= 128,
        "Unsupported normal-launch undo journal"
    );
    for (name, route) in &j.entries {
        ensure!(
            &route.setup.profile == name
                && ["installing", "active", "undoing", "removing_login"]
                    .contains(&route.phase.as_str())
                && route.changes.len() <= 40,
            "Invalid normal-launch journal state"
        );
        valid_id(&route.setup.desktop_id)?;
        if let Some(prepared) = &route.prepared_profile {
            crate::config::validate_profile_input(prepared)?;
            ensure!(
                route.created_profile
                    && prepared.name == *name
                    && content_fingerprint(&serde_json::to_string(prepared)?)
                        == route.profile_fingerprint,
                "Invalid prepared application settings in undo journal"
            );
        }
        ensure!(
            !route.created_profile || route.prepared_profile.is_some(),
            "Prepared application settings are missing from undo journal"
        );
        for change in &route.changes {
            if let Change::Replace { target, .. } = change {
                target.path(paths)?;
            }
        }
    }
    Ok(j)
}
fn save(paths: &Paths, journal: &Journal) -> Result<()> {
    #[derive(Serialize)]
    struct Disk<'a> {
        schema_version: u32,
        entries: &'a BTreeMap<String, Route>,
    }
    let raw = serde_json::to_vec(&Disk {
        schema_version: 2,
        entries: &journal.entries,
    })?;
    ensure!(
        raw.len() <= JOURNAL_LIMIT,
        "Normal-launch undo journal is full"
    );
    crate::fsutil::atomic_private_write(&journal_path(paths), &raw)
}

// Conservative INI edits: preserve all unrelated lines, reject ambiguous or KConfig
// expansion/immutable forms instead of silently overriding policy.
fn key(raw: &str, group: &str, name: &str) -> Result<Option<String>> {
    let mut current = "";
    let mut value = None;
    let mut groups = 0;
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            ensure!(
                !line.starts_with(&format!("[{group}][")),
                "Unsupported immutable configuration group"
            );
            current = line;
            if current == format!("[{group}]") {
                groups += 1;
            }
            continue;
        }
        if current != format!("[{group}]") || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let k = k.trim();
            ensure!(
                !k.starts_with(&format!("{name}[")),
                "Unsupported expanded or immutable desktop setting"
            );
            if k == name {
                ensure!(value.is_none(), "Duplicate desktop setting");
                value = Some(v.trim().into());
            }
        }
    }
    ensure!(groups <= 1, "Duplicate desktop configuration group");
    Ok(value)
}
fn set_key(raw: &str, group: &str, name: &str, value: Option<&str>) -> Result<String> {
    key(raw, group, name)?;
    let header = format!("[{group}]");
    let mut lines: Vec<String> = raw.lines().map(str::to_owned).collect();
    let start = if let Some(i) = lines.iter().position(|s| s.trim() == header) {
        i
    } else {
        if value.is_none() {
            return Ok(raw.into());
        }
        lines.push(header);
        lines.len() - 1
    };
    let end = (start + 1..lines.len())
        .find(|i| lines[*i].trim().starts_with('['))
        .unwrap_or(lines.len());
    let existing = (start + 1..end).find(|i| {
        lines[*i]
            .split_once('=')
            .is_some_and(|(k, _)| k.trim() == name)
    });
    match (existing, value) {
        (Some(i), Some(v)) => lines[i] = format!("{name}={v}"),
        (Some(i), None) => {
            lines.remove(i);
        }
        (None, Some(v)) => lines.insert(end, format!("{name}={v}")),
        _ => {}
    }
    Ok(format!("{}\n", lines.join("\n")))
}
fn current(paths: &Paths, target: &Target) -> Result<Option<String>> {
    let raw = read(&target.path(paths)?, FILE_LIMIT)?;
    match target {
        Target::Mime(name) => key(raw.as_deref().unwrap_or(""), "Default Applications", name),
        _ => Ok(raw),
    }
}
fn write_target(paths: &Paths, target: &Target, value: Option<&str>) -> Result<()> {
    let path = target.path(paths)?;
    if let Target::Mime(name) = target {
        let raw = read(&path, FILE_LIMIT)?.unwrap_or_default();
        write(
            &path,
            Some(&set_key(&raw, "Default Applications", name, value)?),
        )
    } else {
        write(&path, value)
    }
}
fn exclusions(raw: &str) -> Vec<String> {
    raw.split([',', ':'])
        .filter(|s| !s.is_empty())
        .map(|s| s.trim().to_lowercase())
        .collect()
}
fn exclusion_current(paths: &Paths) -> Result<Option<String>> {
    key(
        &read(&session_path(paths)?, FILE_LIMIT)?.unwrap_or_default(),
        "General",
        "excludeApps",
    )
}
fn exclude_value(before: Option<&str>, added: &[String]) -> String {
    let mut result = exclusions(before.unwrap_or(""));
    for token in added {
        if !result.contains(token) {
            result.push(token.clone());
        }
    }
    result.join(",")
}
fn replace_exclusions(
    paths: &Paths,
    before: Option<&str>,
    added: &[String],
    undo: bool,
) -> Result<()> {
    let raw = read(&session_path(paths)?, FILE_LIMIT)?.unwrap_or_default();
    let now = key(&raw, "General", "excludeApps")?;
    let next = if undo {
        if now.as_deref() == Some(&exclude_value(before, added)) {
            before.map(str::to_owned)
        } else {
            Some(
                exclusions(now.as_deref().unwrap_or(""))
                    .into_iter()
                    .filter(|s| !added.contains(s))
                    .collect::<Vec<_>>()
                    .join(","),
            )
        }
    } else {
        Some(exclude_value(now.as_deref(), added))
    };
    write(
        &session_path(paths)?,
        Some(&set_key(&raw, "General", "excludeApps", next.as_deref())?),
    )
}

fn managed_entry(raw: &str, profile: &Profile, cli: &Path, login: bool) -> Result<String> {
    let mut launch_profile = profile.clone();
    if launch_profile.desktop.is_none() {
        // Manually saved profiles append forwarded inputs. Match that behavior
        // while retaining the original launcher's URL/file field code.
        if let Some(imported) =
            launchers::parse_entry("source.desktop", Path::new("/source.desktop"), raw)
                .and_then(|e| e.profile)
                .and_then(|p| p.desktop)
        {
            let mut desktop = imported;
            desktop.file_argument = desktop
                .file_argument
                .map(|(_, code)| (profile.args.len(), code));
            launch_profile.desktop = Some(desktop);
        }
    }
    let generated = launch::desktop_entry(&launch_profile, cli)?;
    let mut exec =
        key(&generated, "Desktop Entry", "Exec")?.context("Missing generated command")?;
    // Desktop launches may forward to an existing identity-registered instance.
    exec = exec.replacen(" launch ", " launch --normal ", 1);
    if login {
        if let Some((_, code)) = launch_profile
            .desktop
            .as_ref()
            .and_then(|d| d.file_argument)
        {
            exec = exec.trim_end_matches(&format!(" -- %{code}")).into();
        }
    }
    let mut result = set_key(
        raw,
        "Desktop Entry",
        "X-Sidecar-Profile",
        Some(&profile.name),
    )?;
    for (name, value) in [
        ("Exec", Some(exec.as_str())),
        ("DBusActivatable", Some("false")),
        ("PrefersNonDefaultGPU", Some("false")),
        ("X-KDE-RunOnDiscreteGpu", Some("false")),
        ("Actions", None),
        ("Hidden", Some("false")),
        ("TryExec", None),
    ] {
        result = set_key(&result, "Desktop Entry", name, value)?;
    }
    // Do not leave action sections that can bypass the managed Exec.
    let mut keep = true;
    let mut output = String::new();
    for line in result.lines() {
        if line.trim().starts_with('[') {
            keep = !line.trim().starts_with("[Desktop Action ");
        }
        if keep {
            output.push_str(line);
            output.push('\n');
        }
    }
    if login {
        output = set_key(
            &output,
            "Desktop Entry",
            "X-GNOME-Autostart-enabled",
            Some("true"),
        )?;
    }
    ensure!(output.len() <= FILE_LIMIT, "Managed launcher is too large");
    Ok(output)
}
fn startup_matches(paths: &Paths, profile: &Profile, id: &str) -> Result<Vec<String>> {
    let mut roots = vec![config_root(paths)?.join("autostart")];
    roots.extend(
        std::env::split_paths(
            &std::env::var_os("XDG_CONFIG_DIRS").unwrap_or_else(|| "/etc/xdg".into()),
        )
        .filter(|p| p.is_absolute())
        .map(|p| p.join("autostart")),
    );
    let mut seen = BTreeSet::new();
    let mut matches = Vec::new();
    for root in roots {
        let entries = match fs::read_dir(&root) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e).context("Cannot inspect login entries"),
        };
        for (count, entry) in entries.enumerate() {
            ensure!(count < 1024, "Too many login entries to inspect safely");
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".desktop") || !seen.insert(name.clone()) {
                continue;
            }
            let raw =
                read(&entry.path(), FILE_LIMIT)?.context("Login entry vanished; preview again")?;
            if key(&raw, "Desktop Entry", "Hidden")?.as_deref() == Some("true") {
                continue;
            }
            let parsed = launchers::parse_entry(&name, &entry.path(), &raw);
            if name == id
                || parsed
                    .and_then(|e| e.profile)
                    .is_some_and(|p| launchers::same_executable(&p.executable, &profile.executable))
            {
                valid_id(&name)?;
                matches.push(name);
            }
        }
    }
    ensure!(matches.len() <= 24, "Too many matching login entries");
    matches.sort();
    Ok(matches)
}
fn build(paths: &Paths, config: &Config, setup: &Setup) -> Result<Route> {
    ensure!(
        !setup.login,
        "App login startup has been removed. Normal launches do not start applications at login"
    );
    valid_id(&setup.desktop_id)?;
    let profile = config
        .profiles
        .iter()
        .find(|p| p.name == setup.profile)
        .context("Save or Keep a profile first")?;
    ensure!(
        !config
            .managed_elsewhere
            .iter()
            .any(|e| launchers::same_executable(e, &profile.executable)),
        "This application is managed elsewhere"
    );
    ensure!(
        setup.cli_path.is_absolute()
            && launchers::executable_path(&setup.cli_path.to_string_lossy()).is_some(),
        "Select an installed absolute Sidecar CLI path"
    );
    ensure!(
        !profile.record && !profile.mangohud,
        "Normal launches require a profile without automatic recording"
    );
    let j = load(paths)?;
    ensure!(
        j.entries.len() < 128 && !j.entries.contains_key(&setup.profile),
        "This profile is already managed; use Check, Finish setup or Undo"
    );
    for route in j.entries.values() {
        ensure!(
            !launchers::same_executable(&route.executable, &profile.executable),
            "This executable is already managed by another profile"
        );
        ensure!(
            route.setup.desktop_id != setup.desktop_id,
            "This desktop launcher is already managed by another profile"
        );
        ensure!(
            !setup.default_browser || !route.setup.default_browser,
            "Undo the other managed default browser first"
        );
    }
    let entry = launchers::installed()
        .into_iter()
        .find(|e| e.id == setup.desktop_id)
        .context("Installed launcher is unavailable; refresh the application list")?;
    let imported = entry
        .profile
        .context("This launcher's command is unsupported")?;
    ensure!(
        launchers::same_executable(&imported.executable, &profile.executable),
        "Launcher executable does not match the saved profile"
    );
    ensure!(
        (profile.desktop.is_none()
            || profile.desktop.as_ref().and_then(|d| d.file_argument)
                == imported.desktop.as_ref().and_then(|d| d.file_argument)),
        "Import this launcher as a profile first to preserve file/URL argument positions"
    );
    let source = Path::new(&entry.path);
    ensure!(
        !source.starts_with(&paths.applications)
            || source == paths.applications.join(&setup.desktop_id),
        "Nested user launchers need manual consolidation before normal-launch management"
    );
    if setup.default_browser {
        ensure!(
            imported
                .desktop
                .as_ref()
                .and_then(|d| d.file_argument)
                .is_some_and(|(_, code)| matches!(code, 'u' | 'U')),
            "Default-browser integration needs a launcher with a URL argument field"
        );
    }
    let raw = read(Path::new(&entry.path), FILE_LIMIT)?.context("Launcher disappeared")?;
    let target = Target::Launcher(setup.desktop_id.clone());
    let mut managed = managed_entry(&raw, profile, &setup.cli_path, false)?;
    if setup.default_browser {
        let mut mime_types = key(&managed, "Desktop Entry", "MimeType")?.unwrap_or_default();
        for name in BROWSER_TYPES {
            if !mime_types.split(';').any(|v| v == *name) {
                if !mime_types.is_empty() && !mime_types.ends_with(';') {
                    mime_types.push(';');
                }
                mime_types.push_str(name);
                mime_types.push(';');
            }
        }
        managed = set_key(&managed, "Desktop Entry", "MimeType", Some(&mime_types))?;
    }
    let mut changes = vec![Change::Replace {
        before: current(paths, &target)?,
        after: managed,
        target,
    }];
    let mut warnings = vec!["Existing applications keep their GPU settings. Close them normally and relaunch to verify. Commands, scripts, direct D-Bus calls and other app IDs can bypass this launcher.".into()];
    if key(&raw, "Desktop Entry", "Actions")?.is_some() {
        warnings.push("Desktop action submenus are omitted from the managed launcher. Undo restores the original actions.".into());
    }
    if setup.default_browser {
        ensure!(
            profile.firefox || launch::is_firefox(&profile.executable),
            "Default-browser integration currently supports native Firefox profiles"
        );
        for name in BROWSER_TYPES {
            let target = Target::Mime((*name).into());
            changes.push(Change::Replace {
                before: current(paths, &target)?,
                after: format!("{};", setup.desktop_id),
                target,
            });
        }
    }
    warnings.push("Session restoration and custom commands can use a different launch path. Verify the app after opening it normally.".into());

    Ok(Route {
        setup: setup.clone(),
        phase: "installing".into(),
        executable: profile.executable.clone(),
        profile_fingerprint: content_fingerprint(&serde_json::to_string(profile)?),
        source_fingerprint: content_fingerprint(&raw),
        source_path: (source != paths.applications.join(&setup.desktop_id))
            .then(|| source.to_path_buf()),
        changes,
        warnings,
        created_profile: false,
        prepared_profile: None,
    })
}
// Fixed FNV-1a for source change detection across compiler updates, not security.
fn content_fingerprint(raw: &str) -> String {
    let hash = raw.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    });
    format!("{hash:016x}")
}

fn preview_of(route: &Route) -> Result<Preview> {
    let bytes = serde_json::to_vec(route)?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    Ok(Preview {
        setup: route.setup.clone(),
        token: format!("{:016x}", h.finish()),
        changes: route
            .changes
            .iter()
            .map(|c| match c {
                Change::Replace { target, .. } => target.label(),
                Change::Exclude { .. } => {
                    "Exclude this application from Plasma session restore".into()
                }
            })
            .collect(),
        warnings: route.warnings.clone(),
    })
}
pub fn preview(paths: &Paths, config: &Config, setup: &Setup) -> Result<Preview> {
    preview_of(&build(paths, config, setup)?)
}
pub fn is_managed(paths: &Paths, name: &str) -> Result<bool> {
    Ok(load(paths)?.entries.contains_key(name))
}
pub fn apply(paths: &Paths, config: &Config, setup: &Setup, token: &str) -> Result<Vec<Status>> {
    let route = build(paths, config, setup)?;
    ensure!(
        preview_of(&route)?.token == token,
        "Launch settings changed since preview; review a fresh preview"
    );
    let mut j = load(paths)?;
    j.entries.insert(setup.profile.clone(), route);
    save(paths, &j)?;
    resume(paths, &setup.profile)
}
pub fn resume(paths: &Paths, name: &str) -> Result<Vec<Status>> {
    let mut j = load(paths)?;
    let route = j
        .entries
        .get(name)
        .context("No managed launch setup")?
        .clone();
    ensure!(
        !route.setup.login && route.phase != "removing_login",
        "Remove the previous login-startup changes or Undo this setup before resuming"
    );
    ensure!(
        route.phase != "undoing",
        "Undo is pending; choose Undo again to finish"
    );
    // Preflight every replacement before any writes; user edits are never repair inputs.
    for change in &route.changes {
        if let Change::Replace {
            target,
            before,
            after,
        } = change
        {
            let now = current(paths, target)?;
            ensure!(now == *before || now.as_deref() == Some(after), "Managed launch conflict; preserve your edit and inspect the launch setup before retrying");
        }
    }
    for change in &route.changes {
        match change {
            Change::Replace { target, after, .. } => {
                if current(paths, target)?.as_deref() != Some(after) {
                    write_target(paths, target, Some(after))?;
                }
            }
            Change::Exclude { before, added, .. } => {
                replace_exclusions(paths, before.as_deref(), added, false)?
            }
        }
    }
    j.entries.get_mut(name).unwrap().phase = "active".into();
    save(paths, &j)?;
    crate::logging::event(
        "info",
        "normal_launch.applied",
        serde_json::json!({"login":route.setup.login,"default_browser":route.setup.default_browser}),
    );
    statuses(paths)
}
pub fn undo(paths: &Paths, name: &str) -> Result<Vec<Status>> {
    undo_impl(paths, name, None)
}
pub fn undo_with_config(paths: &Paths, name: &str, config: &mut Config) -> Result<Vec<Status>> {
    undo_impl(paths, name, Some(config))
}
fn undo_impl(paths: &Paths, name: &str, config: Option<&mut Config>) -> Result<Vec<Status>> {
    let mut j = load(paths)?;
    let route = j
        .entries
        .get(name)
        .context("No managed launch setup")?
        .clone();
    for change in &route.changes {
        if let Change::Replace {
            target,
            before,
            after,
        } = change
        {
            let now = current(paths, target)?;
            ensure!(now == *before || now.as_deref() == Some(after), "Undo conflict: a managed setting was edited outside Sidecar. Your edit and the private undo journal were preserved; restore that setting to its managed or original value, then retry Undo");
        }
    }
    j.entries.get_mut(name).unwrap().phase = "undoing".into();
    save(paths, &j)?;
    for change in route.changes.iter().rev() {
        match change {
            Change::Replace { target, before, .. } => {
                if current(paths, target)? != *before {
                    write_target(paths, target, before.as_deref())?;
                }
            }
            Change::Exclude { before, added, .. } => {
                replace_exclusions(paths, before.as_deref(), added, true)?
            }
        }
    }
    if route.created_profile {
        if let Some(config) = config {
            let mut next = config.clone();
            next.profiles.retain(|p| {
                p.name != name
                    || serde_json::to_string(p)
                        .map(|s| content_fingerprint(&s) != route.profile_fingerprint)
                        .unwrap_or(true)
            });
            next.save(paths)?;
            *config = next;
        }
    }
    j.entries.remove(name);
    save(paths, &j)?;
    crate::logging::event("info", "normal_launch.undone", serde_json::json!({}));
    statuses(paths)
}
pub fn statuses(paths: &Paths) -> Result<Vec<Status>> {
    statuses_impl(paths, None)
}
pub fn statuses_for(paths: &Paths, config: &Config) -> Result<Vec<Status>> {
    statuses_impl(paths, Some(config))
}
fn statuses_impl(paths: &Paths, config: Option<&Config>) -> Result<Vec<Status>> {
    let j = load(paths)?;
    j.entries
        .values()
        .map(|route| {
            let mut s = Status {
                profile: route.setup.profile.clone(),
                desktop_id: route.setup.desktop_id.clone(),
                phase: route.phase.clone(),
                launcher_ok: false,
                login_ok: route.setup.login.then_some(true),
                browser_defaults_ok: route.setup.default_browser.then_some(true),
                restore_excluded: None,
                conflicts: Vec::new(),
            };
            if let Some(config)=config {
                let profile=config.profiles.iter().find(|p|p.name==route.setup.profile).map(serde_json::to_string).transpose()?;
                if profile.is_none_or(|p|content_fingerprint(&p)!=route.profile_fingerprint) {s.conflicts.push("Saved profile changed outside managed setup; Undo and review a fresh setup".into());}
            }
            if let Some(source)=&route.source_path {
                if !read(source,FILE_LIMIT).is_ok_and(|raw|raw.is_some_and(|raw|content_fingerprint(&raw) == route.source_fingerprint)) {s.conflicts.push("Underlying installed launcher changed or is unavailable; Undo and review a fresh setup after application updates".into());}
            }
            for change in &route.changes {
                match change {
                    Change::Replace { target, after, .. } => {
                        let result = current(paths, target);
                        let ok = result.as_ref().is_ok_and(|v| v.as_deref() == Some(after));
                        match target {
                            Target::Launcher(_) => s.launcher_ok = ok,
                            Target::Autostart(_) => {
                                s.login_ok = Some(s.login_ok.unwrap_or(true) && ok)
                            }
                            Target::Mime(_) => {
                                s.browser_defaults_ok =
                                    Some(s.browser_defaults_ok.unwrap_or(true) && ok)
                            }
                        }
                        if !ok {
                            s.conflicts.push(format!(
                                "{}: {}",
                                target.label(),
                                if result.is_err() {
                                    "unavailable"
                                } else {
                                    "changed or pending"
                                }
                            ));
                        }
                    }
                    Change::Exclude { required, .. } => {
                        let now = exclusion_current(paths);
                        let tokens = now
                            .as_ref()
                            .ok()
                            .map(|s| exclusions(s.as_deref().unwrap_or("")));
                        let ok = tokens
                            .is_some_and(|tokens| required.iter().all(|t| tokens.contains(t)));
                        s.restore_excluded = Some(ok);
                        if !ok {
                            s.conflicts
                                .push("Plasma restore exclusion changed or unavailable".into());
                        }
                    }
                }
            }
            if route.setup.login && route.changes.iter().any(|c|matches!(c,Change::Replace {target:Target::Autostart(_),..})) {
                let profile=Profile {executable:route.executable.clone(),..Default::default()};
                match startup_matches(paths,&profile,&route.setup.desktop_id) {
                    Ok(ids)=>for id in ids {
                        if !route.changes.iter().any(|c|matches!(c,Change::Replace {target:Target::Autostart(owned),..} if owned==&id)) {
                            s.login_ok=Some(false);s.conflicts.push(format!("Additional matching login entry: {id}; Undo and review a fresh setup"));
                        }
                    },
                    Err(_)=>{s.login_ok=Some(false);s.conflicts.push("Login-entry inspection unavailable; review startup entries before relying on login routing".into());},
                }
            }
            Ok(s)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, Paths) {
        let d = tempfile::tempdir().unwrap();
        let p = Paths {
            config: d.path().join("config/igpu-man/config.toml"),
            data: d.path().join("data/igpu-man"),
            applications: d.path().join("data/applications"),
        };
        (d, p)
    }
    fn route(paths: &Paths, name: &str, id: &str) -> Route {
        let target = Target::Launcher(id.into());
        Route {
            setup: Setup {
                profile: name.into(),
                desktop_id: id.into(),
                cli_path: "/bin/true".into(),
                login: false,
                default_browser: false,
            },
            phase: "installing".into(),
            executable: "/bin/true".into(),
            profile_fingerprint: "fixture".into(),
            source_fingerprint: "fixture".into(),
            source_path: None,
            changes: vec![
                Change::Replace {
                    before: current(paths, &target).unwrap(),
                    after: "managed".into(),
                    target,
                },
                Change::Exclude {
                    before: exclusion_current(paths).unwrap(),
                    added: vec![name.into()],
                    required: vec![name.into()],
                },
            ],
            warnings: vec![],
            created_profile: false,
            prepared_profile: None,
        }
    }
    fn install_intent(paths: &Paths, route: Route) {
        let mut j = load(paths).unwrap();
        j.entries.insert(route.setup.profile.clone(), route);
        save(paths, &j).unwrap();
    }
    #[test]
    fn interrupted_apply_and_undo_are_repeatable_and_restore_original_bytes() {
        let (_d, p) = fixture();
        let target = p.applications.join("fixture.desktop");
        write(&target, Some("original\n# custom metadata\n")).unwrap();
        let route = route(&p, "fixture", "fixture.desktop");
        install_intent(&p, route);
        // Simulate dying after the first artifact, before updating journal phase.
        write(&target, Some("managed")).unwrap();
        assert_eq!(statuses(&p).unwrap()[0].phase, "installing");
        resume(&p, "fixture").unwrap();
        resume(&p, "fixture").unwrap();
        assert!(statuses(&p).unwrap()[0].launcher_ok);
        assert_eq!(exclusion_current(&p).unwrap().as_deref(), Some("fixture"));
        let mut j = load(&p).unwrap();
        j.entries.get_mut("fixture").unwrap().phase = "undoing".into();
        save(&p, &j).unwrap();
        write(&target, Some("original\n# custom metadata\n")).unwrap();
        assert!(resume(&p, "fixture").is_err());
        assert!(undo(&p, "fixture").unwrap().is_empty());
        assert_eq!(
            read(&target, FILE_LIMIT).unwrap().unwrap(),
            "original\n# custom metadata\n"
        );
        assert_eq!(exclusion_current(&p).unwrap(), None);
    }
    #[test]
    fn undo_preserves_external_edits_and_retains_recovery_data() {
        let (_d, p) = fixture();
        install_intent(&p, route(&p, "fixture", "fixture.desktop"));
        resume(&p, "fixture").unwrap();
        write(
            &p.applications.join("fixture.desktop"),
            Some("external edit"),
        )
        .unwrap();
        assert!(undo(&p, "fixture").is_err());
        assert!(is_managed(&p, "fixture").unwrap());
        assert_eq!(
            read(&p.applications.join("fixture.desktop"), FILE_LIMIT)
                .unwrap()
                .as_deref(),
            Some("external edit")
        );
        assert!(!statuses(&p).unwrap()[0].conflicts.is_empty());
    }
    #[test]
    fn exclusion_undo_preserves_other_apps_and_unrelated_settings() {
        let (_d, p) = fixture();
        write(&session_path(&p).unwrap(),Some("[General]\nexcludeApps=existing\nloginMode=restorePreviousLogout\n[Other]\nkeep=true\n")).unwrap();
        install_intent(&p, route(&p, "first", "first.desktop"));
        resume(&p, "first").unwrap();
        install_intent(&p, route(&p, "second", "second.desktop"));
        resume(&p, "second").unwrap();
        undo(&p, "first").unwrap();
        let states = statuses(&p).unwrap();
        assert_eq!(states[0].restore_excluded, Some(true));
        let raw = read(&session_path(&p).unwrap(), FILE_LIMIT)
            .unwrap()
            .unwrap();
        assert_eq!(
            key(&raw, "General", "excludeApps").unwrap().as_deref(),
            Some("existing,second")
        );
        assert_eq!(
            key(&raw, "General", "loginMode").unwrap().as_deref(),
            Some("restorePreviousLogout")
        );
        assert_eq!(key(&raw, "Other", "keep").unwrap().as_deref(), Some("true"));
        undo(&p, "second").unwrap();
        assert_eq!(exclusion_current(&p).unwrap().as_deref(), Some("existing"));
    }
    #[test]
    fn mime_edits_preserve_unrelated_keys_and_refuse_ambiguous_settings() {
        let (_d, p) = fixture();
        let path = config_root(&p).unwrap().join("mimeapps.list");
        write(&path,Some("[Default Applications]\nx-scheme-handler/http=original.desktop;\nimage/png=viewer.desktop;\n")).unwrap();
        let target = Target::Mime("x-scheme-handler/http".into());
        write_target(&p, &target, Some("managed.desktop;")).unwrap();
        let raw = read(&path, FILE_LIMIT).unwrap().unwrap();
        assert_eq!(
            key(&raw, "Default Applications", "image/png")
                .unwrap()
                .as_deref(),
            Some("viewer.desktop;")
        );
        assert!(key(
            "[General]\nexcludeApps=x\nexcludeApps=y\n",
            "General",
            "excludeApps"
        )
        .is_err());
        assert!(key("[General]\nexcludeApps[$i]=x\n", "General", "excludeApps").is_err());
        assert!(key("[General][$i]\nexcludeApps=x\n", "General", "excludeApps").is_err());
    }
    #[test]
    fn checks_report_profile_and_packaged_launcher_drift_without_repairing_them() {
        let (dir, paths) = fixture();
        let profile = Profile {
            name: "fixture".into(),
            executable: "/bin/true".into(),
            gpu: "0000:01:00.0".into(),
            ..Default::default()
        };
        let mut config = Config::default();
        config.profiles.push(profile.clone());
        let source = dir.path().join("upstream.desktop");
        fs::write(&source, "upstream").unwrap();
        let mut route = route(&paths, "fixture", "fixture.desktop");
        route.profile_fingerprint = content_fingerprint(&serde_json::to_string(&profile).unwrap());
        route.source_path = Some(source.clone());
        route.source_fingerprint = content_fingerprint("upstream");
        install_intent(&paths, route);
        resume(&paths, "fixture").unwrap();
        assert!(statuses_for(&paths, &config).unwrap()[0]
            .conflicts
            .is_empty());
        fs::write(&source, "package update").unwrap();
        config.profiles[0].gpu = "0000:02:00.0".into();
        assert_eq!(statuses_for(&paths, &config).unwrap()[0].conflicts.len(), 2);
        assert_eq!(fs::read_to_string(&source).unwrap(), "package update");
        assert_eq!(
            fs::read_to_string(paths.applications.join("fixture.desktop")).unwrap(),
            "managed"
        );
    }
    #[test]
    fn prepared_profile_recovers_before_launcher_and_undo_removes_only_owned_settings() {
        let (_d, paths) = fixture();
        let mut r = route(&paths, "fixture", "fixture.desktop");
        let profile = Profile {
            name: "fixture".into(),
            executable: "/bin/true".into(),
            gpu: "0000:01:00.0".into(),
            ..Default::default()
        };
        r.created_profile = true;
        r.profile_fingerprint = content_fingerprint(&serde_json::to_string(&profile).unwrap());
        r.prepared_profile = Some(profile);
        install_intent(&paths, r);
        let parent = paths.config.parent().unwrap();
        fs::create_dir_all(parent.parent().unwrap()).unwrap();
        fs::write(parent, "blocked").unwrap();
        let mut config = Config::default();
        assert!(resume_with_config(&paths, "fixture", &mut config).is_err());
        assert!(config.profiles.is_empty() && !paths.applications.join("fixture.desktop").exists());
        assert_eq!(statuses(&paths).unwrap()[0].phase, "installing");
        fs::remove_file(parent).unwrap();
        resume_with_config(&paths, "fixture", &mut config).unwrap();
        resume_with_config(&paths, "fixture", &mut config).unwrap();
        assert_eq!(config.profiles.len(), 1);
        config.profiles.push(Profile {
            name: "unrelated".into(),
            executable: "/bin/true".into(),
            gpu: "0000:01:00.0".into(),
            ..Default::default()
        });
        config.save(&paths).unwrap();
        undo_with_config(&paths, "fixture", &mut config).unwrap();
        assert_eq!(config.profiles.len(), 1);
        assert_eq!(config.profiles[0].name, "unrelated");
        assert!(!paths.applications.join("fixture.desktop").exists());
    }
    #[test]
    fn retiring_v1_login_changes_keeps_routing_and_preserves_other_startup_settings() {
        let (_d, paths) = fixture();
        let mut r = route(&paths, "fixture", "fixture.desktop");
        r.setup.login = true;
        r.changes.push(Change::Replace {
            target: Target::Autostart("fixture.desktop".into()),
            before: None,
            after: "owned startup".into(),
        });
        write(&paths.applications.join("fixture.desktop"), Some("managed")).unwrap();
        write(
            &config_root(&paths)
                .unwrap()
                .join("autostart/fixture.desktop"),
            Some("owned startup"),
        )
        .unwrap();
        write(
            &config_root(&paths)
                .unwrap()
                .join("autostart/unrelated.desktop"),
            Some("unrelated startup"),
        )
        .unwrap();
        write(
            &session_path(&paths).unwrap(),
            Some("[General]\nexcludeApps=fixture,other\nloginMode=restorePreviousLogout\n"),
        )
        .unwrap();
        // The old on-disk shape has no prepared-profile ownership fields.
        let mut value = serde_json::to_value(&r).unwrap();
        value.as_object_mut().unwrap().remove("created_profile");
        value.as_object_mut().unwrap().remove("prepared_profile");
        crate::fsutil::atomic_private_write(
            &journal_path(&paths),
            serde_json::to_vec(
                &serde_json::json!({"schema_version":1,"entries":{"fixture":value}}),
            )
            .unwrap()
            .as_slice(),
        )
        .unwrap();
        assert!(resume(&paths, "fixture").is_err());
        remove_login(&paths, "fixture").unwrap();
        remove_login(&paths, "fixture").unwrap();
        assert_eq!(
            read(&paths.applications.join("fixture.desktop"), FILE_LIMIT)
                .unwrap()
                .as_deref(),
            Some("managed")
        );
        assert!(!config_root(&paths)
            .unwrap()
            .join("autostart/fixture.desktop")
            .exists());
        assert_eq!(
            read(
                &config_root(&paths)
                    .unwrap()
                    .join("autostart/unrelated.desktop"),
                FILE_LIMIT
            )
            .unwrap()
            .as_deref(),
            Some("unrelated startup")
        );
        assert_eq!(exclusion_current(&paths).unwrap().as_deref(), Some("other"));
        assert!(statuses(&paths).unwrap()[0].login_ok.is_none());
        assert!(statuses(&paths).unwrap()[0].launcher_ok);
    }
    #[test]
    fn retiring_login_conflicts_preserve_the_launcher_and_recovery_journal() {
        let (_d, paths) = fixture();
        let mut r = route(&paths, "fixture", "fixture.desktop");
        r.setup.login = true;
        r.changes.push(Change::Replace {
            target: Target::Autostart("fixture.desktop".into()),
            before: None,
            after: "owned".into(),
        });
        install_intent(&paths, r);
        let target = config_root(&paths)
            .unwrap()
            .join("autostart/fixture.desktop");
        write(&target, Some("external edit")).unwrap();
        assert!(remove_login(&paths, "fixture").is_err());
        assert!(is_managed(&paths, "fixture").unwrap());
        assert_eq!(
            read(&target, FILE_LIMIT).unwrap().as_deref(),
            Some("external edit")
        );
    }
    #[test]
    fn unsafe_destinations_and_corrupted_journals_fail_without_writes() {
        let (d, p) = fixture();
        for id in [
            "../evil.desktop",
            "/tmp/x.desktop",
            "igpu-man.desktop",
            "x.desktop\nExec=evil",
        ] {
            assert!(valid_id(id).is_err());
        }
        fs::create_dir_all(&p.applications).unwrap();
        let victim = d.path().join("victim");
        fs::write(&victim, "untouched").unwrap();
        std::os::unix::fs::symlink(&victim, p.applications.join("fixture.desktop")).unwrap();
        assert!(write(&p.applications.join("fixture.desktop"), Some("bad")).is_err());
        assert_eq!(fs::read_to_string(&victim).unwrap(), "untouched");
        crate::fsutil::atomic_private_write(&journal_path(&p), b"{broken").unwrap();
        assert!(statuses(&p).is_err());
        assert!(undo(&p, "fixture").is_err());
    }
    #[test]
    fn managed_launcher_preserves_identity_and_forwards_urls_without_bypass_actions() {
        let raw="[Desktop Entry]\nType=Application\nName=Fixture\nName[fr]=Essai\nIcon=fixture\nExec=/usr/bin/printf %U\nDBusActivatable=true\nPrefersNonDefaultGPU=true\nActions=unsafe;\n[Desktop Action unsafe]\nExec=/usr/bin/printf bypass\n";
        let p = Profile {
            name: "fixture".into(),
            executable: "/usr/bin/printf".into(),
            gpu: "igpu".into(),
            ..Default::default()
        };
        let managed = managed_entry(raw, &p, Path::new("/tmp/a b/igpu-man"), false).unwrap();
        assert_eq!(
            key(&managed, "Desktop Entry", "Name[fr]")
                .unwrap()
                .as_deref(),
            Some("Essai")
        );
        assert_eq!(
            key(&managed, "Desktop Entry", "DBusActivatable")
                .unwrap()
                .as_deref(),
            Some("false")
        );
        let exec = key(&managed, "Desktop Entry", "Exec").unwrap().unwrap();
        assert!(exec.ends_with(" launch --normal fixture -- %U"));
        assert!(!managed.contains("Desktop Action"));
        let login = managed_entry(raw, &p, Path::new("/tmp/igpu-man"), true).unwrap();
        assert!(!key(&login, "Desktop Entry", "Exec")
            .unwrap()
            .unwrap()
            .contains("%U"));
    }
}

/// The primary workflow takes an application, not user-authored profile settings.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AppSetup {
    pub desktop_id: String,
    pub cli_path: PathBuf,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppPreview {
    pub application: String,
    pub setup: AppSetup,
    pub profile: String,
    pub token: String,
    pub warnings: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppApply {
    pub setup: AppSetup,
    pub token: String,
}

fn app_plan(
    paths: &Paths,
    config: &Config,
    setup: &AppSetup,
    gpu: &str,
) -> Result<(Route, Config, String)> {
    valid_id(&setup.desktop_id)?;
    let entry = launchers::installed()
        .into_iter()
        .find(|e| e.id == setup.desktop_id)
        .context("This application is unavailable; refresh the app list")?;
    let mut profile = entry
        .profile
        .context("This application's launcher needs a custom setup in Advanced")?;
    let candidates: Vec<_> = config
        .profiles
        .iter()
        .filter(|p| {
            p.gpu == gpu
                && !p.record
                && !p.mangohud
                && !p.vulkan_only
                && p.args == profile.args
                && p.environment == profile.environment
                && p.working_directory == profile.working_directory
                && launchers::same_executable(&p.executable, &profile.executable)
                && (if p.desktop.is_none() {
                    profile
                        .desktop
                        .as_ref()
                        .and_then(|d| d.file_argument)
                        .is_none_or(|(index, _)| index == p.args.len())
                } else {
                    p.desktop.as_ref().and_then(|d| d.file_argument)
                        == profile.desktop.as_ref().and_then(|d| d.file_argument)
                })
        })
        .collect();
    let created = candidates.len() != 1;
    if !created {
        profile = candidates[0].clone();
    } else {
        let stem: String = setup
            .desktop_id
            .trim_end_matches(".desktop")
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .take(50)
            .collect();
        let base = format!("app-{stem}");
        profile.name = (0..10000)
            .map(|n| {
                if n == 0 {
                    base.clone()
                } else {
                    format!("{base}-{n}")
                }
            })
            .find(|name| !config.profiles.iter().any(|p| p.name == *name))
            .context("Cannot allocate app settings; review Advanced settings")?;
        profile.gpu = gpu.into();
        profile.verification = None;
    }
    let mut next = config.clone();
    if created {
        next.profiles.push(profile.clone());
    }
    let settings = Setup {
        profile: profile.name.clone(),
        desktop_id: setup.desktop_id.clone(),
        cli_path: setup.cli_path.clone(),
        login: false,
        default_browser: false,
    };
    let mut route = build(paths, &next, &settings)?;
    route.created_profile = created;
    route.prepared_profile = created.then_some(profile);
    Ok((route, next, entry.name))
}
pub fn app_preview(
    paths: &Paths,
    config: &Config,
    setup: &AppSetup,
    gpu: &str,
) -> Result<AppPreview> {
    let (route, _, application) = app_plan(paths, config, setup, gpu)?;
    Ok(AppPreview {
        application,
        setup: setup.clone(),
        profile: route.setup.profile.clone(),
        token: preview_of(&route)?.token,
        warnings: route.warnings,
    })
}
pub fn app_apply(
    paths: &Paths,
    config: &mut Config,
    setup: &AppSetup,
    gpu: &str,
    token: &str,
) -> Result<Vec<Status>> {
    let (route, _, _) = app_plan(paths, config, setup, gpu)?;
    ensure!(
        preview_of(&route)?.token == token,
        "Application settings changed. Review the app again before applying"
    );
    let name = route.setup.profile.clone();
    let mut j = load(paths)?;
    j.entries.insert(name.clone(), route);
    save(paths, &j)?;
    resume_with_config(paths, &name, config)
}
/// Commit a journaled new profile before publishing a launcher that references it.
pub fn resume_with_config(paths: &Paths, name: &str, config: &mut Config) -> Result<Vec<Status>> {
    let j = load(paths)?;
    let route = j
        .entries
        .get(name)
        .context("App setup is no longer present")?;
    ensure!(
        !route.setup.login && route.phase != "undoing" && route.phase != "removing_login",
        "Finish removing login changes or Undo before resuming app setup"
    );
    if let Some(prepared) = &route.prepared_profile {
        crate::config::validate_profile_input(prepared)?;
        if let Some(existing) = config.profiles.iter().find(|p| p.name == name) {
            ensure!(
                content_fingerprint(&serde_json::to_string(existing)?) == route.profile_fingerprint,
                "Saved settings changed; Undo this app setup before retrying"
            );
        } else {
            let mut next = config.clone();
            next.profiles.push(prepared.clone());
            next.save(paths)?;
            *config = next;
        }
    }
    resume(paths, name)
}
fn login_change(change: &Change) -> bool {
    matches!(
        change,
        Change::Exclude { .. }
            | Change::Replace {
                target: Target::Autostart(_),
                ..
            }
    )
}
/// Retirement only: never creates app autostart or session-restore exclusions.
pub fn remove_login(paths: &Paths, name: &str) -> Result<Vec<Status>> {
    let mut j = load(paths)?;
    let route = j.entries.get(name).context("No app setup")?.clone();
    ensure!(route.phase != "undoing", "Finish Undo first");
    let changes: Vec<_> = route.changes.iter().filter(|c| login_change(c)).collect();
    for change in &changes {
        if let Change::Replace {
            target,
            before,
            after,
        } = change
        {
            let now = current(paths, target)?;
            ensure!(now==*before || now.as_deref()==Some(after),"A previous startup setting was edited outside Sidecar. Your edit and undo data are preserved");
        }
    }
    j.entries.get_mut(name).unwrap().phase = "removing_login".into();
    save(paths, &j)?;
    for change in changes.into_iter().rev() {
        match change {
            Change::Replace { target, before, .. } => {
                write_target(paths, target, before.as_deref())?
            }
            Change::Exclude { before, added, .. } => {
                replace_exclusions(paths, before.as_deref(), added, true)?
            }
        }
    }
    let route = j.entries.get_mut(name).unwrap();
    route.changes.retain(|c| !login_change(c));
    route.setup.login = false;
    route.phase=if route.changes.iter().all(|c|matches!(c,Change::Replace {target,after,..} if current(paths,target).is_ok_and(|v|v.as_deref()==Some(after)))) {"active"} else {"installing"}.into();
    save(paths, &j)?;
    crate::logging::event("info", "normal_launch.login_removed", serde_json::json!({}));
    statuses(paths)
}
