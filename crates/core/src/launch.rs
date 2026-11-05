use crate::{collector::read_process_stat, config::validate_profile, model::*};
use anyhow::{bail, Context, Result};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub fn resolve_gpu<'a>(gpus: &'a [Gpu], selector: &str) -> Result<&'a Gpu> {
    let matches: Vec<_> = gpus
        .iter()
        .filter(|g| g.pci == selector || g.alias == selector)
        .collect();
    if matches.len() != 1 {
        bail!("GPU '{selector}' is missing or ambiguous; use its PCI address from igpu-man status");
    }
    Ok(matches[0])
}

pub fn environment(
    gpu: &Gpu,
    vulkan_only: bool,
    firefox: bool,
) -> Result<BTreeMap<String, String>> {
    if !valid_pci(&gpu.pci) {
        bail!("Invalid PCI identity");
    }
    let mut env = BTreeMap::new();
    env.insert(
        "DRI_PRIME".into(),
        format!(
            "pci-{}{}",
            gpu.pci.replace([':', '.'], "_"),
            if vulkan_only { "!" } else { "" }
        ),
    );
    if firefox {
        env.insert(
            "MOZ_DRM_DEVICE".into(),
            gpu.render_node.clone().context("GPU has no render node")?,
        );
    }
    Ok(env)
}

fn valid_pci(value: &str) -> bool {
    value.len() == 12
        && value.as_bytes()[4] == b':'
        && value.as_bytes()[7] == b':'
        && value.as_bytes()[10] == b'.'
        && value
            .chars()
            .enumerate()
            .all(|(i, c)| [4, 7, 10].contains(&i) || c.is_ascii_hexdigit())
}

pub fn is_firefox(executable: &str) -> bool {
    let name = Path::new(executable)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    ["firefox", "librewolf", "floorp", "zen-browser", "waterfox"]
        .iter()
        .any(|s| name.starts_with(s))
}

pub fn firefox_running(proc: &Path, own_pid: u32) -> bool {
    use std::os::unix::fs::MetadataExt;
    let uid = unsafe { libc::geteuid() };
    fs::read_dir(proc)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                return false;
            };
            if pid == own_pid
                || fs::metadata(entry.path())
                    .map(|m| m.uid() != uid)
                    .unwrap_or(true)
            {
                return false;
            }
            let exe = fs::read_link(entry.path().join("exe")).unwrap_or_default();
            is_firefox(&exe.to_string_lossy())
                || fs::read_to_string(entry.path().join("comm")).is_ok_and(|s| is_firefox(s.trim()))
        })
}

pub fn launch_command(
    executable: &str,
    args: &[OsString],
    env: &BTreeMap<String, String>,
) -> Command {
    let mut command = Command::new(executable);
    command.args(args).envs(env);
    command
}

pub fn current_process() -> Result<ProcessId> {
    let pid = std::process::id();
    read_process_stat(&PathBuf::from(format!("/proc/{pid}/stat")), pid)
        .map(|p| p.0)
        .context("Cannot identify launch process")
}

/// Only compare executable identities. Never collect process arguments/environments.
pub fn application_running(proc: &Path, executable: &str, own_pid: u32) -> bool {
    use std::os::unix::fs::MetadataExt;
    let uid = unsafe { libc::geteuid() };
    fs::read_dir(proc)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            if entry.file_name().to_string_lossy().parse::<u32>().ok() == Some(own_pid)
                || fs::metadata(entry.path())
                    .map(|m| m.uid() != uid)
                    .unwrap_or(true)
            {
                return false;
            }
            fs::read_link(entry.path().join("exe"))
                .is_ok_and(|p| crate::launchers::same_executable(&p.to_string_lossy(), executable))
        })
}

pub fn profile_arguments(profile: &Profile, forwarded: &[OsString]) -> Result<Vec<OsString>> {
    let mut args: Vec<_> = profile.args.iter().map(OsString::from).collect();
    if let Some(desktop) = &profile.desktop {
        if let Some((index, code)) = desktop.file_argument {
            if index > args.len() {
                bail!("Invalid file/URL argument position");
            }
            if matches!(code, 'f' | 'u') && forwarded.len() > 1 {
                bail!("This launcher accepts only one file/URL per launch");
            }
            args.splice(index..index, forwarded.iter().cloned());
        } else if !forwarded.is_empty() {
            bail!("The imported launcher does not accept file/URL arguments");
        }
    } else {
        args.extend_from_slice(forwarded);
    }
    Ok(args)
}

fn desktop_value(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

pub fn desktop_entry(profile: &Profile, cli_path: &Path) -> Result<String> {
    validate_profile(profile)?;
    // Desktop Entry Exec quoting is not shell quoting. Escape reserved fields,
    // percent codes, and the second layer of backslashes explicitly.
    let cli = cli_path.to_str().context("CLI path must be UTF-8")?;
    if cli.contains(['\n', '\r', '\0']) {
        bail!("Invalid CLI path");
    }
    let quoted = cli
        .replace('\\', "\\\\\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$")
        .replace('%', "%%");
    let display = profile
        .desktop
        .as_ref()
        .map(|d| d.name.as_str())
        .unwrap_or(&profile.name);
    let icon = profile
        .desktop
        .as_ref()
        .map(|d| d.icon.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("igpu-man");
    let categories = profile
        .desktop
        .as_ref()
        .map(|d| d.categories.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("Utility;");
    let forward = profile
        .desktop
        .as_ref()
        .and_then(|d| d.file_argument)
        .map(|(_, code)| format!(" -- %{code}"))
        .unwrap_or_default();
    let mut result = format!("[Desktop Entry]\nType=Application\nName=Sidecar: {}\nExec=\"{}\" launch {}{}\nIcon={}\nTerminal=false\nCategories={}\n", desktop_value(display), quoted, profile.name, forward, desktop_value(icon), desktop_value(categories));
    if let Some(desktop) = &profile.desktop {
        if desktop.file_argument.is_some() && !desktop.mime_types.is_empty() {
            result.push_str(&format!(
                "MimeType={}\n",
                desktop_value(&desktop.mime_types)
            ));
        }
    }
    Ok(result)
}

pub fn steam_command(gpu: &Gpu) -> String {
    format!("igpu-man run --gpu {} -- %command%", gpu.pci)
}

/// Config is the commit record; launchers are recoverable derived artifacts.
/// The durable intent is cleared only after atomic, no-clobber publication.
pub struct KeepOutcome {
    pub config: crate::config::Config,
    pub launcher: Option<PathBuf>,
    pub warning: Option<String>,
}
/// Publish an explicit saved-profile shortcut without ever replacing an existing
/// file (including symlinks). All callers share the same atomic publication path.
pub fn create_profile_launcher(
    paths: &crate::config::Paths,
    profile: &Profile,
    cli: &Path,
) -> Result<PathBuf> {
    anyhow::ensure!(cli.is_absolute(), "Launcher requires an absolute CLI path");
    let content = desktop_entry(profile, cli)?;
    let path = paths
        .applications
        .join(format!("igpu-man-{}.desktop", profile.name));
    publish_launcher(&path, &content)?;
    Ok(path)
}

fn publish_launcher(path: &Path, content: &str) -> Result<()> {
    use std::{io::Write, os::unix::fs::PermissionsExt};
    let parent = path.parent().context("Launcher has no parent")?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))?;
    temp.write_all(content.as_bytes())?;
    temp.as_file().sync_all()?;
    temp.persist_noclobber(path)
        .map_err(|e| e.error)
        .context("Launcher already exists or appeared concurrently; it was preserved")?;
    if let Err(error) = fs::File::open(parent).and_then(|f| f.sync_all()) {
        crate::logging::event(
            "warn",
            "profile.launcher_durability_warning",
            crate::logging::error_fields(&error.into()),
        );
    }
    Ok(())
}
pub fn recover_pending_launchers(
    config: &crate::config::Config,
    paths: &crate::config::Paths,
) -> (crate::config::Config, Vec<String>) {
    let mut next = config.clone();
    let mut resolved = false;
    let mut warnings = vec![];
    for profile in &mut next.profiles {
        let Some(pending) = &profile.pending_launcher else {
            continue;
        };
        let path = paths
            .applications
            .join(format!("igpu-man-{}.desktop", profile.name));
        let result = (|| -> Result<()> {
            match fs::symlink_metadata(&path) {
                Ok(metadata) => {
                    anyhow::ensure!(
                        metadata.file_type().is_file(),
                        "Existing launcher is not a regular file"
                    );
                    anyhow::ensure!(
                        crate::fsutil::read_text_limited(&path, 128 * 1024)? == pending.content,
                        "Existing launcher differs; it was preserved"
                    );
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    publish_launcher(&path, &pending.content)?
                }
                Err(error) => return Err(error.into()),
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                profile.pending_launcher = None;
                resolved = true;
            }
            Err(error) => {
                crate::logging::event(
                    "warn",
                    "profile.launcher_recovery_pending",
                    crate::logging::error_fields(&error),
                );
                warnings.push("A saved profile's launcher needs repair; an existing conflicting launcher was preserved. Inspect collector logs.".into());
            }
        }
    }
    if resolved {
        if let Err(error) = next.save(paths) {
            crate::logging::event(
                "warn",
                "profile.launcher_intent_cleanup_failed",
                crate::logging::error_fields(&error),
            );
            warnings.push("Launcher publication completed; durable intent cleanup will retry at the next startup.".into());
            return (config.clone(), warnings);
        }
        crate::logging::event(
            "info",
            "profile.launcher_recovery_completed",
            serde_json::json!({}),
        );
    }
    (next, warnings)
}
pub fn keep_profile(
    config: &crate::config::Config,
    paths: &crate::config::Paths,
    mut profile: Profile,
    cli_path: Option<&Path>,
) -> Result<KeepOutcome> {
    validate_profile(&profile)?;
    anyhow::ensure!(
        !config.profiles.iter().any(|p| p.name == profile.name),
        "Profile name already exists; choose another name"
    );
    let launcher = if let Some(cli) = cli_path {
        anyhow::ensure!(
            cli.is_absolute(),
            "Launcher must reference an absolute igpu-man executable path"
        );
        let path = paths
            .applications
            .join(format!("igpu-man-{}.desktop", profile.name));
        match fs::symlink_metadata(&path) {
            Ok(_) => bail!("Launcher already exists; choose another profile name"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        profile.pending_launcher = Some(PendingLauncher {
            schema_version: 1,
            content: desktop_entry(&profile, cli)?,
        });
        Some(path)
    } else {
        profile.pending_launcher = None;
        None
    };
    let mut next = config.clone();
    next.managed_elsewhere
        .retain(|p| !crate::launchers::same_executable(p, &profile.executable));
    next.profiles.push(profile);
    next.save(paths)?; // Before this commit no launcher is visible; after it the intent is recoverable.
    let (next, warnings) = recover_pending_launchers(&next, paths);
    let available = launcher.filter(|p| {
        p.is_file()
            && next
                .profiles
                .last()
                .is_some_and(|p| p.pending_launcher.is_none())
    });
    Ok(KeepOutcome {
        config: next,
        launcher: available,
        warning: (!warnings.is_empty()).then(|| warnings.join(" ")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn separate_launcher_preserves_files_symlinks_and_concurrent_winners() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::config::Paths {
            config: dir.path().join("config.toml"),
            data: dir.path().into(),
            applications: dir.path().join("applications"),
        };
        let profile = Profile {
            name: "shortcut".into(),
            executable: "/bin/true".into(),
            ..Default::default()
        };
        let cli = Path::new("/tmp/igpu-man");
        assert!(create_profile_launcher(&paths, &profile, Path::new("relative")).is_err());
        assert!(!paths.applications.exists());
        let path = create_profile_launcher(&paths, &profile, cli).unwrap();
        let original = fs::read(&path).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(create_profile_launcher(&paths, &profile, cli).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        fs::remove_file(&path).unwrap();
        let target = dir.path().join("original");
        fs::write(&target, "user-owned").unwrap();
        symlink(&target, &path).unwrap();
        assert!(create_profile_launcher(&paths, &profile, cli).is_err());
        assert!(fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read_to_string(&target).unwrap(), "user-owned");
        fs::remove_file(&path).unwrap();
        // A dangling symlink is also an existing destination.
        fs::remove_file(&target).unwrap();
        symlink(&target, &path).unwrap();
        assert!(create_profile_launcher(&paths, &profile, cli).is_err());
        assert!(!target.exists());
        fs::remove_file(&path).unwrap();
        let gate = std::sync::Barrier::new(2);
        let outcomes = std::thread::scope(|scope| {
            let a = scope.spawn(|| {
                gate.wait();
                create_profile_launcher(&paths, &profile, cli)
            });
            let b = scope.spawn(|| {
                gate.wait();
                create_profile_launcher(&paths, &profile, cli)
            });
            [a.join().unwrap(), b.join().unwrap()]
        });
        assert_eq!(outcomes.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::read_dir(&paths.applications).unwrap().count(), 1);
    }

    #[test]
    fn interrupted_keep_reconciles_before_and_after_publish_without_clobbering_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::config::Paths {
            config: dir.path().join("config.toml"),
            data: dir.path().into(),
            applications: dir.path().join("applications"),
        };
        let mut profile = Profile {
            name: "recover".into(),
            executable: "/bin/true".into(),
            ..Default::default()
        };
        let content = desktop_entry(&profile, Path::new("/tmp/igpu-man")).unwrap();
        profile.pending_launcher = Some(PendingLauncher {
            schema_version: 1,
            content: content.clone(),
        });
        let config = crate::config::Config {
            profiles: vec![profile],
            ..Default::default()
        };
        config.save(&paths).unwrap(); // Crash after committing intent, before publication.
        let (recovered, warnings) = recover_pending_launchers(&config, &paths);
        assert!(warnings.is_empty());
        assert!(recovered.profiles[0].pending_launcher.is_none());
        let file = paths.applications.join("igpu-man-recover.desktop");
        assert_eq!(fs::read_to_string(&file).unwrap(), content);
        config.save(&paths).unwrap(); // Crash after publication, before intent cleanup.
        let (recovered, warnings) = recover_pending_launchers(&config, &paths);
        assert!(warnings.is_empty() && recovered.profiles[0].pending_launcher.is_none());
        config.save(&paths).unwrap();
        fs::write(&file, "user launcher must remain").unwrap();
        let (recovered, warnings) = recover_pending_launchers(&config, &paths);
        assert!(!warnings.is_empty() && recovered.profiles[0].pending_launcher.is_some());
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "user launcher must remain"
        );
        assert!(crate::config::Config::load(&paths).unwrap().profiles[0]
            .pending_launcher
            .is_some());
        fs::remove_file(&file).unwrap();
        std::os::unix::fs::symlink("/tmp/missing-user-launcher", &file).unwrap();
        assert!(!recover_pending_launchers(&config, &paths).1.is_empty());
        assert!(file.is_symlink());
    }
    #[test]
    fn imported_file_arguments_keep_their_position_and_literal_contents() {
        let mut p = Profile {
            name: "test".into(),
            executable: "/usr/bin/printf".into(),
            args: vec!["prefix".into(), "suffix".into()],
            desktop: Some(DesktopMetadata {
                id: "test.desktop".into(),
                name: "Test".into(),
                icon: "app".into(),
                mime_types: "text/plain;".into(),
                categories: "Utility;".into(),
                file_argument: Some((1, 'U')),
            }),
            ..Profile::default()
        };
        let forwarded = [
            OsString::from("file:///tmp/a b"),
            OsString::from("https://example.invalid/a?x=$(echo)\"%U"),
        ];
        let args = profile_arguments(&p, &forwarded).unwrap();
        assert_eq!(
            args,
            [
                OsString::from("prefix"),
                forwarded[0].clone(),
                forwarded[1].clone(),
                OsString::from("suffix")
            ]
        );
        let entry = desktop_entry(&p, Path::new("/tmp/cli with space")).unwrap();
        assert!(entry.contains("launch test -- %U\n"));
        assert!(entry.contains("Icon=app\n"));
        assert!(entry.contains("MimeType=text/plain;\n"));
        p.desktop.as_mut().unwrap().file_argument = Some((1, 'u'));
        assert!(profile_arguments(&p, &forwarded).is_err());
    }
    #[test]
    fn keep_preserves_existing_launchers_and_rolls_back_failed_configuration_save() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::config::Paths {
            config: dir.path().join("config/config.toml"),
            data: dir.path().join("data"),
            applications: dir.path().join("applications"),
        };
        let p = Profile {
            name: "test".into(),
            executable: "/usr/bin/printf".into(),
            ..Profile::default()
        };
        let config = crate::config::Config::default();
        let outcome =
            keep_profile(&config, &paths, p.clone(), Some(Path::new("/tmp/igpu-man"))).unwrap();
        let (next, launcher) = (outcome.config, outcome.launcher);
        let path = launcher.unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(
            keep_profile(&config, &paths, p.clone(), Some(Path::new("/tmp/igpu-man"))).is_err()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), content);
        assert!(keep_profile(&next, &paths, p.clone(), None).is_err());
        let block = dir.path().join("blocked");
        fs::write(&block, "file").unwrap();
        let paths = crate::config::Paths {
            config: block.join("config.toml"),
            ..paths
        };
        let p = Profile {
            name: "rollback".into(),
            ..p
        };
        assert!(keep_profile(&config, &paths, p, Some(Path::new("/tmp/igpu-man"))).is_err());
        assert!(!paths
            .applications
            .join("igpu-man-rollback.desktop")
            .exists());
    }
    #[test]
    fn generic_running_detection_uses_executable_identity() {
        assert!(crate::launchers::same_executable(
            "/usr/bin/printf (deleted)",
            "/usr/bin/printf"
        ));
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("42")).unwrap();
        std::os::unix::fs::symlink("/usr/bin/printf", dir.path().join("42/exe")).unwrap();
        assert!(application_running(dir.path(), "/usr/bin/printf", 99));
        assert!(!application_running(dir.path(), "/usr/bin/printf", 42));
        assert!(!application_running(dir.path(), "/usr/bin/true", 99));
    }
    #[test]
    fn preserves_arguments_without_evaluating_shell_text() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("SHOULD_NOT_EXIST");
        let argument = format!("spaces $(touch {}) `echo nope` ; $HOME", marker.display());
        let output = launch_command(
            "/usr/bin/printf",
            &[OsString::from("%s"), OsString::from(&argument)],
            &BTreeMap::new(),
        )
        .output()
        .unwrap();
        assert_eq!(String::from_utf8(output.stdout).unwrap(), argument);
        assert!(!marker.exists());
    }
    #[test]
    fn selection_is_pci_based_and_ambiguity_fails() {
        let gpu = Gpu {
            pci: "0000:0e:00.0".into(),
            name: "test".into(),
            display_name: None,
            display_name_source: None,
            alias: "igpu".into(),
            integrated: true,
            card: "card9".into(),
            render_node: Some("/dev/dri/renderD129".into()),
            capabilities: vec![],
            outputs: vec![],
            metrics: BTreeMap::new(),
        };
        let env = environment(&gpu, true, true).unwrap();
        assert_eq!(env["DRI_PRIME"], "pci-0000_0e_00_0!");
        assert_eq!(env["MOZ_DRM_DEVICE"], "/dev/dri/renderD129");
        assert!(resolve_gpu(&[gpu.clone(), gpu.clone()], "igpu").is_err());
        assert_eq!(resolve_gpu(&[gpu], "igpu").unwrap().pci, "0000:0e:00.0");
    }
    #[test]
    fn firefox_detection_excludes_unrelated_processes() {
        let dir = tempfile::tempdir().unwrap();
        let proc = dir.path().join("12");
        fs::create_dir(&proc).unwrap();
        fs::write(proc.join("comm"), "firefox\n").unwrap();
        assert!(firefox_running(dir.path(), 99));
        assert!(!firefox_running(dir.path(), 12));
        fs::write(proc.join("comm"), "browser-helper").unwrap();
        assert!(!firefox_running(dir.path(), 99));
    }
}

/// An ordinary desktop link may reach an already managed Firefox. Registration
/// is launch intent, not proof of current renderer/decoder placement. All live
/// Firefox-family identities must be covered; unknown/reused instances refuse.
pub fn registered_firefox_instance(
    snapshot: &Snapshot,
    profile: &Profile,
    proc: &Path,
    own_pid: u32,
) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Ok(entries) = fs::read_dir(proc) else {
        return false;
    };
    let mut seen = false;
    for entry in entries {
        let Ok(entry) = entry else {
            return false;
        };
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        if pid == own_pid
            || !fs::metadata(entry.path()).is_ok_and(|m| m.uid() == unsafe { libc::geteuid() })
        {
            continue;
        }
        let executable = fs::read_link(entry.path().join("exe")).unwrap_or_default();
        let firefox = is_firefox(&executable.to_string_lossy())
            || fs::read_to_string(entry.path().join("comm")).is_ok_and(|s| is_firefox(s.trim()));
        if !firefox {
            continue;
        }
        let Some((id, ..)) = read_process_stat(&entry.path().join("stat"), pid) else {
            return false;
        };
        if !snapshot.applications.iter().any(|app| {
            app.launch_id.is_some()
                && app.profile_name.as_deref() == Some(&profile.name)
                && app.requested_gpu.as_deref() == Some(&profile.gpu)
                && app.processes.iter().any(|p| p.id == id)
        }) {
            return false;
        }
        seen = true;
    }
    seen
}

#[cfg(test)]
mod normal_instance_tests {
    use super::*;
    #[test]
    fn desktop_forwarding_requires_all_live_firefox_pid_start_identities() {
        let d = tempfile::tempdir().unwrap();
        let process = d.path().join("99");
        fs::create_dir(&process).unwrap();
        std::os::unix::fs::symlink("/usr/lib/firefox/firefox", process.join("exe")).unwrap();
        fs::write(process.join("comm"), "firefox").unwrap();
        fs::write(
            process.join("stat"),
            "99 (firefox) S 1 0 0 0 0 0 0 0 0 0 12 13 0 0 0 0 0 0 456 0",
        )
        .unwrap();
        let profile = Profile {
            name: "browser".into(),
            executable: "firefox".into(),
            gpu: "0000:01:00.0".into(),
            ..Default::default()
        };
        let mut collector = crate::collector::Collector::new(
            d.path(),
            d.path(),
            Diagnostics {
                session: "fixture".into(),
                kwin_support: None,
                display_configuration: None,
                versions: Default::default(),
                notes: vec![],
            },
        );
        let mut sample = collector.sample(&crate::config::Config::default());
        assert!(!registered_firefox_instance(&sample, &profile, d.path(), 1));
        let id = ProcessId {
            pid: 99,
            start_ticks: 456,
        };
        let cpu = Metric::measured(0.0, "%", "fixture");
        sample.applications.push(Application {
            id: id.clone(),
            launch_id: Some(id.clone()),
            profile_name: Some("browser".into()),
            name: "Firefox".into(),
            executable: "firefox".into(),
            processes: vec![Process {
                id: id.clone(),
                parent: 1,
                executable: "firefox".into(),
                name: "firefox".into(),
                cpu_percent: cpu.clone(),
            }],
            requested_gpu: Some(profile.gpu.clone()),
            observed_gpus: vec![],
            placement: "unknown".into(),
            gpus: Default::default(),
            cpu_percent: cpu,
        });
        assert!(registered_firefox_instance(&sample, &profile, d.path(), 1));
        sample.applications[0].processes[0].id.start_ticks = 455;
        assert!(!registered_firefox_instance(&sample, &profile, d.path(), 1));
        sample.applications[0].processes[0].id.start_ticks = 456;
        sample.applications[0].requested_gpu = Some("other".into());
        assert!(!registered_firefox_instance(&sample, &profile, d.path(), 1));
        sample.applications[0].requested_gpu = Some(profile.gpu.clone());
        let other = d.path().join("100");
        fs::create_dir(&other).unwrap();
        fs::write(other.join("comm"), "firefox").unwrap();
        fs::write(
            other.join("stat"),
            "100 (firefox) S 1 0 0 0 0 0 0 0 0 0 12 13 0 0 0 0 0 0 999 0",
        )
        .unwrap();
        assert!(!registered_firefox_instance(&sample, &profile, d.path(), 1));
    }
}
