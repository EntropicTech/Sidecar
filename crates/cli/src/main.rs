use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use igpu_core::{
    client,
    collector::Collector,
    config::{Config, Paths},
    launch, launchers,
    model::*,
};
use serde_json::{json, Value};
use std::{
    ffi::OsString,
    fs,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
};

#[derive(Parser)]
#[command(
    version,
    long_version=igpu_core::build::LONG_VERSION,
    about = "Observe GPU placement, launch explicit profiles, and compare recordings"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Private build-tool bridge; JSON arrives on stdin, never in process arguments.
    #[command(hide = true)]
    InternalRequest {
        operation: String,
    },
    /// Read recent private structured logs. Output is a JSON array.
    Logs {
        #[arg(long, default_value="all", value_parser=["all","cli","daemon","desktop"])]
        component: String,
        #[arg(long, default_value_t=100, value_parser=clap::value_parser!(u16).range(1..=1000))]
        lines: u16,
    },
    Status {
        #[arg(long)]
        json: bool,
        #[arg(long, help = "Sample locally without the service")]
        direct: bool,
    },
    Doctor,
    Candidates {
        #[arg(long)]
        json: bool,
        #[arg(long)]
        all: bool,
    },
    Launchers {
        #[arg(long)]
        json: bool,
    },
    Manage {
        executable: String,
        #[arg(long, conflicts_with = "restore", required_unless_present = "restore")]
        elsewhere: bool,
        #[arg(long)]
        restore: bool,
    },
    /// Internal explicit trial launch; no profile is persisted.
    #[command(hide = true)]
    TryProfile {
        profile_json: String,
    },
    Run {
        #[arg(long)]
        gpu: String,
        #[arg(long)]
        vulkan_only: bool,
        #[arg(long)]
        firefox: bool,
        #[arg(long)]
        record: bool,
        #[arg(long, requires = "record")]
        mangohud: bool,
        #[arg(required = true, last = true)]
        command: Vec<OsString>,
    },
    Launch {
        profile: String,
        #[arg(last = true)]
        arguments: Vec<OsString>,
    },
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    Record {
        #[command(subcommand)]
        command: RecordCommand,
    },
    Mark {
        #[arg(default_value = "stutter happened")]
        label: String,
    },
    Experiment,
    Benchmark {
        #[arg(long, default_value_t = 30)]
        seconds: u64,
        #[arg(long, default_value_t = 1000)]
        interval_ms: u64,
    },
}

#[derive(Subcommand)]
enum ProfileCommand {
    List,
    Import {
        name: String,
        desktop_id: String,
        #[arg(long, default_value = "igpu")]
        gpu: String,
    },
    Save {
        name: String,
        #[arg(long)]
        gpu: String,
        #[arg(long)]
        vulkan_only: bool,
        #[arg(long)]
        firefox: bool,
        #[arg(long)]
        record: bool,
        #[arg(long, requires = "record")]
        mangohud: bool,
        #[arg(required = true, last = true)]
        command: Vec<String>,
    },
    Delete {
        name: String,
    },
    Launcher {
        name: String,
    },
    Steam {
        #[arg(long)]
        gpu: String,
    },
}

#[derive(Subcommand)]
enum RecordCommand {
    Start {
        #[arg(default_value = "manual")]
        label: String,
    },
    Stop {
        #[arg(long)]
        expected_id: Option<i64>,
    },
    List,
    Show {
        id: i64,
    },
    Notes {
        id: i64,
        notes: String,
    },
    Import {
        id: i64,
        file: PathBuf,
        #[arg(long, help = "Collector monotonic timestamp for elapsed=0 in this log")]
        anchor_ns: Option<u64>,
        #[arg(long, requires = "anchor_ns")]
        uncertainty_ms: Option<f64>,
    },
    Export {
        id: i64,
        directory: PathBuf,
    },
    Delete {
        id: i64,
    },
    Compare {
        #[arg(required = true, num_args = 2..)]
        ids: Vec<i64>,
    },
}

fn rpc(method: &str, payload: Value) -> Result<Value> {
    client::call(method, &payload)
}
fn print(value: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
fn snapshot() -> Result<Snapshot> {
    let snapshot: Snapshot = client::call("snapshot", &json!({}))?;
    if snapshot.api_version != igpu_core::API_VERSION {
        bail!("Incompatible collector API");
    }
    Ok(snapshot)
}

fn main() -> std::process::ExitCode {
    igpu_core::logging::init("cli");
    let result = run();
    if let Err(error) = &result {
        igpu_core::logging::event(
            "error",
            "cli.failed",
            igpu_core::logging::error_fields(error),
        );
    }
    if let Err(error) = result {
        eprintln!(
            "igpu-man: {}",
            igpu_core::logging::diagnostic_message(&error)
        );
        std::process::ExitCode::FAILURE
    } else {
        std::process::ExitCode::SUCCESS
    }
}

fn run() -> Result<()> {
    let cli = Cli::try_parse().unwrap_or_else(|error| {
        if error.use_stderr() {
            igpu_core::logging::event(
                "warn",
                "cli.arguments_rejected",
                json!({"kind":format!("{:?}",error.kind())}),
            );
        }
        error.exit()
    });
    match cli.command {
        Commands::InternalRequest { operation } => {
            use std::io::Read;
            let mut bytes = Vec::new();
            std::io::stdin()
                .take(1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            anyhow::ensure!(bytes.len() <= 1024 * 1024, "Internal request exceeds 1 MiB");
            let payload: Value = serde_json::from_slice(&bytes)?;
            let result: Value = client::call(&operation, &payload)?;
            print(&result)?;
        }
        Commands::Logs { component, lines } => print(&serde_json::to_value(
            igpu_core::logging::tail(&component, lines as usize)?,
        )?)?,
        Commands::Candidates { json: as_json, all } => {
            let candidates: Vec<igpu_core::applications::Candidate> =
                client::call("candidates", &json!({}))?;
            if as_json {
                print(&serde_json::to_value(candidates)?)?;
            } else {
                for c in candidates.into_iter().filter(|c| {
                    all || matches!(
                        c.management,
                        igpu_core::applications::Management::Candidate
                            | igpu_core::applications::Management::SavedProfile
                    )
                }) {
                    println!("{} [{}] · {:?} · graphics {:?}% · video {:?}% · {:.1}s observed / last 60s · {}", c.name, c.process.pid, c.management, c.graphics_percent, c.video_percent, c.observed_seconds, c.executable);
                    println!(
                        "  {} · {}",
                        c.launch_path.kind.label(),
                        c.launch_path.explanation
                    );
                    if !c.launch_path.matching_profiles.is_empty() {
                        println!("  Available profiles: {} (use igpu-man launch PROFILE after closing the old instance normally)", c.launch_path.matching_profiles.join(", "));
                    }
                }
                println!("DRM client engine times; not whole-device utilization or predicted gaming gains. Use --all for excluded, idle and inaccessible applications.");
            }
        }
        Commands::Launchers { json: as_json } => {
            let entries = launchers::installed();
            if as_json {
                print(&serde_json::to_value(entries)?)?;
            } else {
                for entry in entries {
                    println!(
                        "{} · {}{}",
                        entry.name,
                        entry.id,
                        entry.problem.map(|p| format!(" · {p}")).unwrap_or_default()
                    );
                }
            }
        }
        Commands::Manage {
            executable,
            elsewhere,
            ..
        } => {
            print(&rpc(
                "set_management",
                json!({"executable": executable, "managed_elsewhere": elsewhere}),
            )?)?;
        }
        Commands::TryProfile { profile_json } => {
            let profile: Profile = serde_json::from_str(&profile_json)?;
            igpu_core::config::validate_profile(&profile)?;
            if profile.record || profile.mangohud {
                bail!("Placement trials do not start recordings or MangoHud");
            }
            if launch::application_running(
                Path::new("/proc"),
                &profile.executable,
                std::process::id(),
            ) {
                bail!("This application is already running. Close it normally before testing a fresh launch.");
            }
            let args = launch::profile_arguments(&profile, &[])?;
            execute(profile, args)?;
        }
        Commands::Status {
            json: as_json,
            direct,
        } => {
            let s = if direct {
                let paths = Paths::discover()?;
                Collector::default().sample(&Config::load(&paths)?)
            } else {
                snapshot()?
            };
            if as_json {
                print(&serde_json::to_value(s)?)?;
            } else {
                println!(
                    "igpu-man API {} · {} · recording {:?}",
                    s.api_version, s.diagnostics.session, s.active_recording
                );
                for gpu in &s.gpus {
                    println!(
                        "{}  {}  {}  {}",
                        gpu.alias,
                        gpu.pci,
                        gpu.name,
                        gpu.render_node.as_deref().unwrap_or("no render node")
                    );
                    for (name, metric) in &gpu.metrics {
                        println!(
                            "  {name}: {} {} ({:?})",
                            metric
                                .value
                                .map(|v| format!("{v:.2}"))
                                .unwrap_or_else(|| "—".into()),
                            metric.unit,
                            metric.status
                        );
                    }
                    for output in &gpu.outputs {
                        if output.connected {
                            println!(
                                "  output {} {:?} {:?} Hz scale {:?}",
                                output.connector,
                                output.current_mode,
                                output.refresh_hz,
                                output.scale
                            );
                        }
                    }
                }
                for app in &s.applications {
                    println!(
                        "{} [{}] requested {:?} · active {:?} · {}",
                        app.name, app.id.pid, app.requested_gpu, app.observed_gpus, app.placement
                    );
                }
                println!("Coverage: {} accessible user processes / {} system processes seen; {} permission failures; {} unique DRM clients. Process totals are partial.", s.coverage.processes_inspected, s.coverage.processes_seen, s.coverage.permission_denied, s.coverage.clients_observed);
                for recommendation in &s.recommendations {
                    println!("• {recommendation}");
                }
            }
        }
        Commands::Doctor => {
            let paths = Paths::discover()?;
            let config = Config::load(&paths)?;
            let mut collector = Collector::default();
            let s = collector.sample(&config);
            print(
                &json!({"session": s.diagnostics, "gpus": s.gpus, "coverage": s.coverage,
                "service": snapshot().map(|_| "reachable".to_owned()).unwrap_or_else(|e| format!("{e:#}")),
                "config": paths.config, "recordings": paths.data,
                "application_version": env!("CARGO_PKG_VERSION"),
                "build":igpu_core::build::metadata(),
                "executable_sha256":igpu_core::build::executable_hash().map(Value::String).unwrap_or_else(|error|json!({"unavailable":igpu_core::logging::diagnostic_message(&error)})),
                "collector_build":client::call::<Value>("capabilities",&json!({})).map(|v|v.get("build").cloned().unwrap_or(json!({"unavailable":"older collector"}))).unwrap_or_else(|error|json!({"unavailable":igpu_core::logging::diagnostic_message(&error)})),
                "dashboard": client::dashboard_status().unwrap_or_else(|_|json!({"running":"unknown","reason":"session bus/control unavailable"})),
                "logging": {"directory": igpu_core::logging::directory().ok(),"schema_version":1,"file_limit_mib":5,"rotations":3,"retained_inactive_files":32},
                "config_summary": {"sample_interval_ms":config.sample_interval_ms,"recording_interval_ms":config.recording_interval_ms,"storage_limit_mb":config.storage_limit_mb,"profiles":config.profiles.len(),"managed_elsewhere":config.managed_elsewhere.len()},
                "firefox_checklist": ["Launch Firefox through a profile after fully exiting existing Firefox instances.",
                    "Inspect about:support → Graphics and Media for WebRender and codec capabilities.",
                    "Play a known video and inspect Firefox's video/decode engine deltas on each GPU.",
                    "Compare browser CPU activity and manually note playback drops/quality.",
                    "Missing decode activity does not prove software decoding; the engine or process may be unobservable.",
                    "Cross-GPU transfers cannot be established from open render nodes alone."]}),
            )?;
        }
        Commands::Run {
            gpu,
            vulkan_only,
            firefox,
            record,
            mangohud,
            command,
        } => {
            let executable = command[0]
                .to_str()
                .context("Executable path must be UTF-8")?
                .to_owned();
            execute(
                Profile {
                    name: "adhoc".into(),
                    executable: executable.clone(),
                    args: vec![],
                    gpu,
                    vulkan_only,
                    firefox: firefox || launch::is_firefox(&executable),
                    record,
                    mangohud,
                    ..Profile::default()
                },
                command[1..].to_vec(),
            )?;
        }
        Commands::Launch { profile, arguments } => {
            let profiles: Vec<Profile> = client::call("profiles", &json!({}))?;
            let profile = profiles
                .into_iter()
                .find(|p| p.name == profile)
                .context("Unknown profile")?;
            let args = launch::profile_arguments(&profile, &arguments)?;
            execute(profile, args)?;
        }
        Commands::Profile { command } => match command {
            ProfileCommand::List => print(&rpc("profiles", json!({}))?)?,
            ProfileCommand::Import {
                name,
                desktop_id,
                gpu,
            } => {
                let entry = launchers::installed()
                    .into_iter()
                    .find(|e| e.id == desktop_id)
                    .context("Installed launcher not found")?;
                let mut profile = entry.profile.with_context(|| {
                    entry
                        .problem
                        .unwrap_or_else(|| "Unsupported launcher".into())
                })?;
                profile.name = name;
                profile.gpu = gpu;
                let profiles: Vec<Profile> = client::call("profiles", &json!({}))?;
                if profiles.iter().any(|p| p.name == profile.name) {
                    bail!("A profile with this name already exists");
                }
                print(&rpc("save_profile", serde_json::to_value(profile)?)?)?;
            }
            ProfileCommand::Save {
                name,
                gpu,
                vulkan_only,
                firefox,
                record,
                mangohud,
                command,
            } => {
                let profile = Profile {
                    name,
                    executable: command[0].clone(),
                    args: command[1..].to_vec(),
                    gpu,
                    vulkan_only,
                    firefox: firefox || launch::is_firefox(&command[0]),
                    record,
                    mangohud,
                    ..Profile::default()
                };
                print(&rpc("save_profile", serde_json::to_value(profile)?)?)?;
            }
            ProfileCommand::Delete { name } => {
                print(&rpc("delete_profile", json!({"name": name}))?)?
            }
            ProfileCommand::Launcher { name } => {
                let profiles: Vec<Profile> = client::call("profiles", &json!({}))?;
                let profile = profiles
                    .iter()
                    .find(|p| p.name == name)
                    .context("Unknown profile")?;
                let paths = Paths::discover()?;
                fs::create_dir_all(&paths.applications)?;
                let file = paths
                    .applications
                    .join(format!("igpu-man-{}.desktop", profile.name));
                fs::write(
                    &file,
                    launch::desktop_entry(profile, &std::env::current_exe()?)?,
                )?;
                println!("{}", file.display());
            }
            ProfileCommand::Steam { gpu } => println!(
                "{}",
                launch::steam_command(launch::resolve_gpu(&snapshot()?.gpus, &gpu)?)
            ),
        },
        Commands::Record { command } => {
            let result = match command {
                RecordCommand::Start { label } => rpc("start", json!({"label": label})),
                RecordCommand::Stop { expected_id } => {
                    rpc("stop", json!({"expected_id":expected_id}))
                }
                RecordCommand::List => rpc("recordings", json!({})),
                RecordCommand::Show { id } => rpc("detail", json!({"id": id})),
                RecordCommand::Notes { id, notes } => {
                    rpc("notes", json!({"id": id, "notes": notes}))
                }
                RecordCommand::Import {
                    id,
                    file,
                    anchor_ns,
                    uncertainty_ms,
                } => {
                    if fs::metadata(&file)?.len() > 64 * 1024 * 1024 {
                        bail!("Log exceeds 64 MiB limit");
                    }
                    rpc(
                        "import",
                        json!({"id": id, "filename": file.file_name().unwrap_or_default().to_string_lossy(),
                        "raw": igpu_core::fsutil::read_text_limited(&file,64*1024*1024)?, "anchor_ns": anchor_ns, "uncertainty_ms": uncertainty_ms}),
                    )
                }
                RecordCommand::Export { id, directory } => {
                    let directory = if directory.is_absolute() {
                        directory
                    } else {
                        std::env::current_dir()?.join(directory)
                    };
                    rpc("export", json!({"id": id, "destination": directory}))
                }
                RecordCommand::Delete { id } => rpc("delete_recording", json!({"id": id})),
                RecordCommand::Compare { ids } => rpc("compare", json!({"ids": ids})),
            }?;
            print(&result)?;
        }
        Commands::Mark { label } => print(&rpc("mark", json!({"label": label}))?)?,
        Commands::Experiment => print(&rpc("experiment", json!({}))?)?,
        Commands::Benchmark {
            seconds,
            interval_ms,
        } => benchmark(seconds, interval_ms)?,
    }
    Ok(())
}

fn find_program(program: &str) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let executable = |p: &Path| {
        fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    if program.contains('/') {
        let path = PathBuf::from(program);
        if executable(&path) {
            return Ok(path);
        }
    } else if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(program);
            if executable(&candidate) {
                return Ok(candidate);
            }
        }
    }
    bail!("Executable '{program}' not found or not executable")
}

fn execute(mut profile: Profile, args: Vec<OsString>) -> Result<()> {
    if profile.record {
        client::require_feature("guarded_stop")?;
    }
    let s = snapshot()?;
    let gpu = launch::resolve_gpu(&s.gpus, &profile.gpu)?;
    profile.gpu = gpu.pci.clone();
    let firefox = profile.firefox || launch::is_firefox(&profile.executable);
    if firefox && launch::firefox_running(Path::new("/proc"), std::process::id()) {
        bail!("Firefox is already running. Fully exit it, then launch this profile so the new GPU settings take effect. No processes were terminated.");
    }
    find_program(&profile.executable)?;
    let mut environment = profile.environment.clone();
    // Explicit GPU selection wins over GPU variables in the original launcher.
    environment.extend(launch::environment(gpu, profile.vulkan_only, firefox)?);
    let working_directory = profile.working_directory.clone();
    let mut executable = profile.executable.clone();
    let mut arguments = args;
    let mut directory = None;
    if profile.mangohud {
        find_program("mangohud")
            .context("Install MangoHud (and lib32-mangohud for 32-bit games) first")?;
        if executable == "gamescope" {
            bail!("gamescope requires mangoapp; direct MangoHud wrapping is unsupported in v0.1");
        }
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .context("XDG_RUNTIME_DIR is needed for temporary MangoHud logs")?;
        let path = runtime.join("igpu-man").join(format!(
            "frames-{}-{}",
            std::process::id(),
            monotonic_ns()
        ));
        fs::create_dir_all(&path)?;
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path.parent().unwrap(), fs::Permissions::from_mode(0o700))?;
        let path_text = path.to_str().context("MangoHud log path must be UTF-8")?;
        if path_text.contains([',', '\n', '\r']) {
            bail!("Unsupported MangoHud output path");
        }
        // Capture an experiment-sized, bounded log. Telemetry can continue longer.
        environment.insert("MANGOHUD_CONFIG".into(), format!("autostart_log=1,log_duration=120,log_interval=0,output_folder={path_text},permit_upload=0"));
        arguments.insert(0, OsString::from(&executable));
        executable = "mangohud".into();
        directory = Some(path.to_string_lossy().into_owned());
    }
    let started: Option<i64> = if profile.record && s.active_recording.is_none() {
        Some(client::call("start", &json!({"label": profile.name}))?)
    } else {
        None
    };
    let registration = LaunchRegistration {
        process: launch::current_process()?,
        gpu: gpu.pci.clone(),
        profile: Some(profile),
        mangohud_directory: directory,
        anchor_ns: monotonic_ns(),
    };
    if let Err(error) = client::call::<bool>("register_launch", &registration) {
        if let Some(id) = started {
            if let Err(cleanup) = rpc("stop", json!({"expected_id":id})) {
                igpu_core::logging::event(
                    "error",
                    "launch.recording_cleanup_failed",
                    igpu_core::logging::error_fields(&cleanup),
                );
            }
        }
        return Err(error);
    }
    let mut command = launch::launch_command(&executable, &arguments, &environment);
    igpu_core::logging::event(
        "info",
        "launch.registered",
        json!({"process":registration.process,"gpu":gpu.pci,"recording_id":started}),
    );
    if let Some(directory) = working_directory {
        command.current_dir(directory);
    }
    let error = command.exec();
    if let Some(id) = started {
        if let Err(cleanup) = rpc("stop", json!({"expected_id":id})) {
            igpu_core::logging::event(
                "error",
                "launch.recording_cleanup_failed",
                igpu_core::logging::error_fields(&cleanup),
            );
        }
    }
    Err(error).with_context(|| format!("Could not execute {executable}"))
}

fn benchmark(seconds: u64, interval_ms: u64) -> Result<()> {
    if seconds == 0 || seconds > 3600 || interval_ms < 100 {
        bail!("Use 1–3600 seconds and intervals >= 100 ms");
    }
    let paths = Paths::discover()?;
    let config = Config::load(&paths)?;
    let mut collector = Collector::default();
    // /proc/self/stat reports CPU ticks without requiring a platform-specific clock binding in this binary.
    let ticks = || {
        igpu_core::collector::read_process_stat(Path::new("/proc/self/stat"), std::process::id())
            .map(|s| s.2)
            .unwrap_or(0)
    };
    let start_ticks = ticks();
    let started = std::time::Instant::now();
    let mut samples = 0u64;
    let mut duration = 0.0;
    while started.elapsed().as_secs_f64() < seconds as f64 {
        let start = std::time::Instant::now();
        let sample = collector.sample(&config);
        duration += sample.collector_duration_ms;
        samples += 1;
        std::thread::sleep(
            std::time::Duration::from_millis(interval_ms).saturating_sub(start.elapsed()),
        );
    }
    let elapsed = started.elapsed().as_secs_f64();
    let hz = igpu_core::collector::clock_ticks_per_second();
    print(
        &json!({"mode": "collector-only; diagnostics initialization excluded", "seconds": elapsed,
        "samples": samples, "interval_ms": interval_ms, "mean_sample_ms": duration / samples as f64,
        "cpu_percent_one_core": (ticks() - start_ticks) as f64 / hz / elapsed * 100.0,
        "limitations": "Measures this process only. Run multiple times; compare game frametimes with monitoring off, tray-only, dashboard open, and recording."}),
    )
}
