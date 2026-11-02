use crate::{
    config::Config,
    fdinfo::{self, Client, EngineCounters},
    model::*,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub struct Collector {
    sys: PathBuf,
    proc: PathBuf,
    counters: EngineCounters,
    cpu: BTreeMap<ProcessId, (u64, u64)>,
    pub launches: Vec<LaunchRegistration>,
    launch_members: BTreeMap<ProcessId, ProcessId>,
    gpu_names: BTreeMap<String, String>,
    gpu_fds: BTreeMap<ProcessId, (u64, Vec<std::ffi::OsString>)>,
    known_gpus: BTreeSet<String>,
    pub diagnostics: Diagnostics,
}

impl Default for Collector {
    fn default() -> Self {
        Self::new("/sys", "/proc", diagnostics())
    }
}

impl Collector {
    pub fn new(
        sys: impl Into<PathBuf>,
        proc: impl Into<PathBuf>,
        diagnostics: Diagnostics,
    ) -> Self {
        Self {
            sys: sys.into(),
            proc: proc.into(),
            counters: EngineCounters::default(),
            cpu: BTreeMap::new(),
            launches: Vec::new(),
            launch_members: BTreeMap::new(),
            gpu_names: BTreeMap::new(),
            gpu_fds: BTreeMap::new(),
            known_gpus: BTreeSet::new(),
            diagnostics,
        }
    }
    pub fn sample(&mut self, config: &Config) -> Snapshot {
        let started = Instant::now();
        let now = monotonic_ns();
        let mut gpus = discover_gpus(&self.sys, config);
        for gpu in &mut gpus {
            if self.sys == Path::new("/sys") {
                let name = self.gpu_names.entry(gpu.pci.clone()).or_insert_with(|| {
                    command_output("lspci", &["-s", &gpu.pci])
                        .and_then(|s| s.trim().split_once(": ").map(|(_, name)| name.to_owned()))
                        .unwrap_or_else(|| gpu.name.clone())
                });
                gpu.name = name.clone();
            }
        }
        apply_display_configuration(&mut gpus, self.diagnostics.display_configuration.as_ref());
        let known: BTreeSet<_> = gpus.iter().map(|g| g.pci.clone()).collect();
        if known != self.known_gpus {
            self.gpu_fds.clear();
            self.known_gpus = known.clone();
        }
        let mut coverage = Coverage {
            fdinfo_scan_interval_ms: 5000,
            ..Coverage::default()
        };
        let mut processes = BTreeMap::new();
        let mut clients: BTreeMap<(String, u64), (Client, BTreeSet<ProcessId>)> = BTreeMap::new();
        let mut opened: BTreeMap<ProcessId, BTreeSet<String>> = BTreeMap::new();
        let uid = unsafe { libc::geteuid() };
        use std::os::unix::fs::MetadataExt;
        if let Ok(entries) = fs::read_dir(&self.proc) {
            for entry in entries.flatten() {
                let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                    continue;
                };
                let path = entry.path();
                coverage.processes_seen += 1;
                // Only inspect this user's processes. Other users are outside coverage.
                if fs::metadata(&path).map(|m| m.uid() != uid).unwrap_or(true) {
                    continue;
                }
                let Some((id, parent, ticks, name)) = read_process_stat(&path.join("stat"), pid)
                else {
                    coverage.vanished += 1;
                    continue;
                };
                let executable = fs::read_link(path.join("exe"))
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let cpu_source = format!("/proc/{pid}/stat");
                let cpu = match self.cpu.insert(id.clone(), (ticks, now)) {
                    Some((old, time)) if ticks >= old && now > time => {
                        let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as f64;
                        Metric::measured(
                            (ticks - old) as f64 / hz / ((now - time) as f64 / 1e9) * 100.0,
                            "% one CPU core",
                            cpu_source,
                        )
                    }
                    _ => Metric::missing("% one CPU core", cpu_source, Availability::WarmingUp),
                };
                processes.insert(
                    pid,
                    Process {
                        id: id.clone(),
                        parent,
                        executable,
                        name,
                        cpu_percent: cpu,
                    },
                );
                // Discovery is much more expensive than sampling the few GPU
                // clients. New processes are scanned immediately; stable process
                // descriptor lists are refreshed every five seconds.
                let cached = self
                    .gpu_fds
                    .get(&id)
                    .filter(|(time, _)| now.saturating_sub(*time) < 5_000_000_000);
                let scanning = cached.is_none();
                let fds: Vec<std::ffi::OsString> = if let Some((_, fds)) = cached {
                    fds.clone()
                } else {
                    match fs::read_dir(path.join("fdinfo")) {
                        Ok(entries) => entries.flatten().map(|e| e.file_name()).collect(),
                        Err(e) => {
                            count_error(&mut coverage, &e);
                            continue;
                        }
                    }
                };
                coverage.processes_inspected += 1;
                let mut discovered = Vec::new();
                for fd in fds {
                    let text = match fs::read_to_string(path.join("fdinfo").join(&fd)) {
                        Ok(text) => text,
                        Err(e) => {
                            count_error(&mut coverage, &e);
                            continue;
                        }
                    };
                    let Some(client) = fdinfo::parse(&text) else {
                        continue;
                    };
                    if !known.contains(&client.pci) {
                        continue;
                    }
                    discovered.push(fd);
                    opened
                        .entry(id.clone())
                        .or_default()
                        .insert(client.pci.clone());
                    let Some(client_id) = client.id else {
                        coverage.unidentifiable_clients += 1;
                        continue;
                    };
                    let value = clients
                        .entry((client.pci.clone(), client_id))
                        .or_insert_with(|| (client, BTreeSet::new()));
                    value.1.insert(id.clone());
                }
                if scanning {
                    self.gpu_fds.insert(id, (now, discovered));
                }
            }
        }
        self.cpu
            .retain(|id, _| processes.get(&id.pid).is_some_and(|p| p.id == *id));
        self.gpu_fds
            .retain(|id, _| processes.get(&id.pid).is_some_and(|p| p.id == *id));
        // A browser/game launcher can exit while its descendants continue. Preserve
        // known members by PID+start-time; never transfer a registration to a reused PID.
        self.launch_members
            .retain(|id, _| processes.get(&id.pid).is_some_and(|p| p.id == *id));
        for process in processes.values() {
            let root = application_root(process.id.pid, &processes, &self.launches);
            if let Some(launch) = self
                .launches
                .iter()
                .find(|l| processes.get(&root).is_some_and(|p| p.id == l.process))
            {
                self.launch_members
                    .insert(process.id.clone(), launch.process.clone());
            }
        }
        self.launches
            .retain(|l| self.launch_members.values().any(|root| *root == l.process));
        let mut representatives = BTreeMap::<ProcessId, u32>::new();
        for (member, root) in &self.launch_members {
            representatives
                .entry(root.clone())
                .and_modify(|pid| *pid = (*pid).min(member.pid))
                .or_insert(member.pid);
        }
        let roots: BTreeMap<u32, u32> = processes
            .iter()
            .map(|(&pid, p)| {
                let root = self
                    .launch_members
                    .get(&p.id)
                    .and_then(|id| representatives.get(id))
                    .copied()
                    .unwrap_or_else(|| application_root(pid, &processes, &self.launches));
                (pid, root)
            })
            .collect();
        let mut groups: BTreeMap<u32, Application> = BTreeMap::new();
        let gpu_pids: BTreeSet<_> = opened.keys().map(|id| roots[&id.pid]).collect();
        for (&pid, p) in &processes {
            let root = roots[&pid];
            if !gpu_pids.contains(&root) && !self.launches.iter().any(|l| l.process.pid == root) {
                continue;
            }
            let root_process = &processes[&root];
            let registration_root = self.launch_members.get(&root_process.id);
            let registration = self
                .launches
                .iter()
                .find(|l| Some(&l.process) == registration_root);
            let group = groups.entry(root).or_insert_with(|| Application {
                id: root_process.id.clone(),
                launch_id: registration.map(|l| l.process.clone()),
                profile_name: registration
                    .and_then(|l| l.profile.as_ref())
                    .map(|p| p.name.clone()),
                name: root_process.name.clone(),
                executable: root_process.executable.clone(),
                processes: Vec::new(),
                requested_gpu: registration.map(|l| l.gpu.clone()),
                observed_gpus: Vec::new(),
                placement: "unverified".into(),
                gpus: BTreeMap::new(),
                cpu_percent: Metric::missing(
                    "% one CPU core",
                    "sum of process CPU ticks",
                    Availability::WarmingUp,
                ),
            });
            group.processes.push(p.clone());
            if let Some(devices) = opened.get(&p.id) {
                for gpu in devices {
                    group.gpus.entry(gpu.clone()).or_default().opened = true;
                }
            }
        }
        let live = clients.keys().cloned().collect();
        self.counters.retain_clients(&live);
        coverage.clients_observed = clients.len();
        for ((pci, _), (client, owners)) in clients {
            // A shared DRM file is counted once, assigned to the first application
            // root in deterministic PID order. Per-client memory is not GPU total.
            let Some(root) = owners.iter().filter_map(|id| roots.get(&id.pid)).min() else {
                continue;
            };
            let Some(group) = groups.get_mut(root) else {
                continue;
            };
            let usage = group.gpus.entry(pci).or_default();
            for (engine, metric) in self.counters.sample(&client, now) {
                merge_metric(&mut usage.engines, engine, metric);
            }
            for (region, bytes) in client.memory {
                merge_metric(
                    &mut usage.memory,
                    region,
                    Metric::measured(
                        bytes as f64,
                        "bytes (client accounting; shared)",
                        "proc/fdinfo",
                    ),
                );
            }
        }
        let mut recommendations = Vec::new();
        for group in groups.values_mut() {
            let cpu: Vec<_> = group
                .processes
                .iter()
                .filter_map(|p| p.cpu_percent.value)
                .collect();
            if !cpu.is_empty() {
                group.cpu_percent = Metric::measured(
                    cpu.iter().sum(),
                    "% one CPU core",
                    "sum of observed process CPU ticks",
                );
            }
            group.observed_gpus = group
                .gpus
                .iter()
                .filter(|(_, usage)| {
                    usage
                        .engines
                        .values()
                        .any(|m| m.value.is_some_and(|n| n > 0.0))
                })
                .map(|(pci, _)| pci.clone())
                .collect();
            if let Some(target) = &group.requested_gpu {
                if group.observed_gpus.iter().any(|pci| pci != target) {
                    group.placement = "activity_on_other_gpu".into();
                    recommendations.push(format!("{}: GPU engine activity observed outside requested GPU {}. Inspect graphics and video engines separately.", group.name, target));
                } else if group.observed_gpus.contains(target) {
                    group.placement = "activity_on_requested_gpu".into();
                }
            }
        }
        Snapshot {
            api_version: crate::API_VERSION,
            monotonic_ns: now,
            unix_ms: unix_ms(),
            gpus,
            applications: groups.into_values().collect(),
            coverage,
            diagnostics: self.diagnostics.clone(),
            recommendations,
            active_recording: None,
            collector_duration_ms: started.elapsed().as_secs_f64() * 1000.0,
        }
    }
}

fn merge_metric(map: &mut BTreeMap<String, Metric>, key: String, metric: Metric) {
    if let Some(existing) = map.get_mut(&key) {
        match (existing.value, metric.value) {
            (Some(a), Some(b)) => {
                existing.value = Some(a + b);
                existing.status = if a + b == 0.0 {
                    Availability::Idle
                } else {
                    Availability::Available
                };
            }
            _ => {
                existing.value = None;
                existing.status = Availability::Stale;
            }
        }
    } else {
        map.insert(key, metric);
    }
}

fn count_error(coverage: &mut Coverage, error: &io::Error) {
    if error.kind() == io::ErrorKind::PermissionDenied {
        coverage.permission_denied += 1;
    } else if error.kind() == io::ErrorKind::NotFound {
        coverage.vanished += 1;
    }
}

pub fn read_process_stat(path: &Path, pid: u32) -> Option<(ProcessId, u32, u64, String)> {
    let text = fs::read_to_string(path).ok()?;
    let start = text.find('(')?;
    let end = text.rfind(')')?;
    let name = text[start + 1..end].to_owned();
    let fields: Vec<_> = text[end + 1..].split_whitespace().collect();
    Some((
        ProcessId {
            pid,
            start_ticks: fields.get(19)?.parse().ok()?,
        },
        fields.get(1)?.parse().ok()?,
        fields
            .get(11)?
            .parse::<u64>()
            .ok()?
            .saturating_add(fields.get(12)?.parse().ok()?),
        name,
    ))
}

fn application_root(
    pid: u32,
    processes: &BTreeMap<u32, Process>,
    launches: &[LaunchRegistration],
) -> u32 {
    let mut current = pid;
    let mut seen = BTreeSet::new();
    while seen.insert(current) {
        let process = &processes[&current];
        if launches.iter().any(|l| l.process == process.id) {
            return current;
        }
        let Some(parent) = processes.get(&process.parent) else {
            break;
        };
        // Walk descendants to a registered ancestor even through helper executables.
        let mut ancestor = parent;
        let mut ancestry = BTreeSet::new();
        while ancestry.insert(ancestor.id.pid) {
            if launches.iter().any(|l| l.process == ancestor.id) {
                return ancestor.id.pid;
            }
            let Some(next) = processes.get(&ancestor.parent) else {
                break;
            };
            ancestor = next;
        }
        if process.executable.is_empty() || process.executable != parent.executable {
            break;
        }
        current = parent.id.pid;
    }
    current
}

pub fn discover_gpus(sys: &Path, config: &Config) -> Vec<Gpu> {
    let drm = sys.join("class/drm");
    let mut result = Vec::new();
    let Ok(entries) = fs::read_dir(&drm) else {
        return result;
    };
    for entry in entries.flatten() {
        let card = entry.file_name().to_string_lossy().into_owned();
        if !card
            .strip_prefix("card")
            .is_some_and(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
        {
            continue;
        }
        let device = entry.path().join("device");
        if read_trim(&device.join("vendor")).as_deref() != Some("0x1002") {
            continue;
        }
        let Some(pci) = fs::canonicalize(&device)
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        else {
            continue;
        };
        let integrated = read_trim(&device.join("mem_info_vram_total"))
            .and_then(|s| s.parse::<u64>().ok())
            .is_some_and(|n| n <= 2 * 1024 * 1024 * 1024);
        let integrated = config
            .aliases
            .get("igpu")
            .map(|id| id == &pci)
            .unwrap_or(integrated);
        let render_node = fs::read_dir(device.join("drm"))
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .find_map(|e| {
                e.file_name()
                    .to_str()
                    .filter(|s| s.starts_with("renderD"))
                    .map(|s| format!("/dev/dri/{s}"))
            });
        let alias = config
            .aliases
            .iter()
            .find(|(_, id)| **id == pci)
            .map(|(a, _)| a.clone())
            .unwrap_or_else(|| if integrated { "igpu" } else { "dgpu" }.into());
        let mut metrics: BTreeMap<String, Metric> = BTreeMap::new();
        for (name, file, unit, factor) in [
            ("gpu_busy", "gpu_busy_percent", "%", 1.0),
            ("memory_busy", "mem_busy_percent", "%", 1.0),
            ("vram_used", "mem_info_vram_used", "bytes", 1.0),
            ("vram_total", "mem_info_vram_total", "bytes", 1.0),
            ("gtt_used", "mem_info_gtt_used", "bytes", 1.0),
        ] {
            metrics.insert(name.into(), read_metric(&device.join(file), unit, factor));
        }
        let hwmon = fs::read_dir(device.join("hwmon"))
            .ok()
            .and_then(|mut e| e.next())
            .and_then(Result::ok)
            .map(|e| e.path());
        for (name, file, unit, factor) in [
            ("temperature", "temp1_input", "°C", 0.001),
            ("power", "power1_average", "W", 0.000001),
            ("core_clock", "freq1_input", "MHz", 0.000001),
            ("memory_clock", "freq2_input", "MHz", 0.000001),
        ] {
            let mut metric = hwmon
                .as_ref()
                .map(|p| read_metric(&p.join(file), unit, factor))
                .unwrap_or_else(|| {
                    Metric::missing(unit, "hwmon not exposed", Availability::Unavailable)
                });
            if name == "power" && metric.value.is_none() {
                if let Some(p) = &hwmon {
                    metric = read_metric(&p.join("power1_input"), unit, factor);
                }
            }
            metrics.insert(name.into(), metric);
        }
        let mut outputs = Vec::new();
        if let Ok(connectors) = fs::read_dir(&drm) {
            for connector in connectors.flatten() {
                let name = connector.file_name().to_string_lossy().into_owned();
                if !name.starts_with(&format!("{card}-")) {
                    continue;
                }
                outputs.push(Output {
                    connector: name[card.len() + 1..].into(),
                    connected: read_trim(&connector.path().join("status")).as_deref()
                        == Some("connected"),
                    advertised_modes: read_trim(&connector.path().join("modes"))
                        .map(|s| s.lines().map(str::to_owned).collect())
                        .unwrap_or_default(),
                    current_mode: None,
                    refresh_hz: None,
                    scale: None,
                });
            }
        }
        let capabilities = metrics
            .iter()
            .filter(|(_, m)| m.value.is_some())
            .map(|(n, _)| n.clone())
            .collect();
        let product = read_trim(&device.join("product_name")).unwrap_or_else(|| {
            format!(
                "AMD {} ({})",
                if integrated {
                    "integrated GPU (inferred)"
                } else {
                    "GPU"
                },
                read_trim(&device.join("device")).unwrap_or_default()
            )
        });
        result.push(Gpu {
            pci,
            name: product,
            alias,
            integrated,
            card: format!("/dev/dri/{card}"),
            render_node,
            capabilities,
            outputs,
            metrics,
        });
    }
    result.sort_by(|a, b| a.pci.cmp(&b.pci));
    // Ambiguous inferred aliases must not silently select the first GPU.
    let aliases: Vec<_> = result.iter().map(|g| g.alias.clone()).collect();
    for gpu in &mut result {
        if aliases.iter().filter(|a| **a == gpu.alias).count() > 1 {
            gpu.alias = gpu.pci.clone();
        }
    }
    result
}

pub fn read_metric(path: &Path, unit: &str, factor: f64) -> Metric {
    let source = path.to_string_lossy().into_owned();
    match fs::read_to_string(path) {
        Ok(text) => match text.trim().parse::<f64>() {
            Ok(n) if n.is_finite() && n >= 0.0 => Metric::measured(n * factor, unit, source),
            _ => Metric::missing(unit, source, Availability::Unavailable),
        },
        Err(e) => Metric::missing(
            unit,
            source,
            if e.kind() == io::ErrorKind::PermissionDenied {
                Availability::PermissionDenied
            } else {
                Availability::Unavailable
            },
        ),
    }
}

fn read_trim(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().into())
}

pub fn clock_ticks_per_second() -> f64 {
    unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as f64
}

/// Subprocesses are diagnostics only, bounded to avoid hanging the sampler.
pub fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let output_path = std::env::temp_dir().join(format!(
        "igpu-man-diag-{}-{}",
        std::process::id(),
        monotonic_ns()
    ));
    use std::os::unix::fs::OpenOptionsExt;
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&output_path)
        .ok()?;
    let child = Command::new(program)
        .args(args)
        .stdout(file)
        .stderr(Stdio::null())
        .spawn();
    let result = (|| {
        let mut child = child.ok()?;
        let started = Instant::now();
        loop {
            if let Some(status) = child.try_wait().ok()? {
                return status
                    .success()
                    .then(|| fs::read_to_string(&output_path).ok())
                    .flatten();
            }
            if started.elapsed() > Duration::from_secs(2) {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })();
    let _ = fs::remove_file(output_path);
    result.map(|s| s.chars().take(65536).collect())
}

pub fn diagnostics() -> Diagnostics {
    let mut versions = BTreeMap::new();
    for (key, program, args) in [
        ("kernel", "uname", vec!["-r"]),
        ("kwin", "kwin_wayland", vec!["--version"]),
        ("firefox", "firefox", vec!["--version"]),
        ("mangohud", "mangohud", vec!["--version"]),
        ("mesa", "pacman", vec!["-Q", "mesa"]),
    ] {
        versions.insert(
            key.into(),
            command_output(program, &args)
                .unwrap_or_else(|| "unavailable".into())
                .trim()
                .into(),
        );
    }
    let kwin_support = command_output("qdbus6", &["org.kde.KWin", "/KWin", "supportInformation"]);
    let display_configuration =
        command_output("kscreen-doctor", &["-j"]).and_then(|s| serde_json::from_str(&s).ok());
    let mut notes = vec!["Process coverage includes accessible processes owned by this user only; shared DRM clients are counted once. Client memory is not additive physical GPU usage.".into(),
        "GPU descriptors are rediscovered every 5 seconds for existing processes; known clients follow the current sampling interval. New processes are discovered each tick.".into(),
        "Integrated-GPU classification is inferred from VRAM size; use explicit PCI aliases if incorrect.".into(),
        "GPU power sensors may omit system/CPU power; do not interpret their sum as wall power.".into()];
    if kwin_support.is_none() {
        notes.push(
            "KWin support information unavailable (qdbus6 or Plasma session missing).".into(),
        );
    }
    Diagnostics {
        session: std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".into()),
        kwin_support,
        display_configuration,
        versions,
        notes,
    }
}

fn apply_display_configuration(gpus: &mut [Gpu], configuration: Option<&serde_json::Value>) {
    let Some(outputs) = configuration
        .and_then(|v| v.get("outputs"))
        .and_then(|v| v.as_array())
    else {
        return;
    };
    for gpu in gpus {
        for connector in &mut gpu.outputs {
            let Some(output) = outputs
                .iter()
                .find(|o| o["name"].as_str() == Some(&connector.connector))
            else {
                continue;
            };
            connector.scale = output["scale"].as_f64();
            if let Some(mode) = output["modes"]
                .as_array()
                .and_then(|modes| modes.iter().find(|m| m["id"] == output["currentModeId"]))
            {
                connector.current_mode = mode["name"].as_str().map(str::to_owned);
                connector.refresh_hz = mode["refreshRate"].as_f64();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn process_stat_handles_spaces_parentheses_and_start_time() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("stat");
        fs::write(
            &file,
            "99 (name (odd)) S 1 0 0 0 0 0 0 0 0 0 12 13 0 0 0 0 0 0 456 0",
        )
        .unwrap();
        let (id, parent, cpu, name) = read_process_stat(&file, 99).unwrap();
        assert_eq!(
            (id.start_ticks, parent, cpu, name.as_str()),
            (456, 1, 25, "name (odd)")
        );
    }
    #[test]
    fn missing_and_zero_are_different() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sensor");
        assert_eq!(
            read_metric(&file, "%", 1.0).status,
            Availability::Unavailable
        );
        fs::write(&file, "0\n").unwrap();
        assert_eq!(read_metric(&file, "%", 1.0).status, Availability::Idle);
    }
    fn process_fixture(proc: &Path, pid: u32, parent: u32, start: u64, client: &str) {
        let path = proc.join(pid.to_string());
        fs::create_dir_all(path.join("fdinfo")).unwrap();
        fs::write(
            path.join("stat"),
            format!("{pid} (firefox) S {parent} 0 0 0 0 0 0 0 0 0 12 13 0 0 0 0 0 0 {start} 0"),
        )
        .unwrap();
        std::os::unix::fs::symlink("/usr/bin/firefox", path.join("exe")).unwrap();
        fs::write(path.join("fdinfo/4"), client).unwrap();
        fs::write(path.join("fdinfo/5"), client).unwrap();
    }
    fn sys_fixture(sys: &Path) {
        let device = sys.join("devices/0000:0e:00.0");
        fs::create_dir_all(device.join("drm/renderD129")).unwrap();
        fs::write(device.join("vendor"), "0x1002").unwrap();
        fs::write(device.join("device"), "0x13c0").unwrap();
        fs::write(device.join("mem_info_vram_total"), "536870912").unwrap();
        let drm = sys.join("class/drm");
        fs::create_dir_all(&drm).unwrap();
        fs::create_dir_all(drm.join("card8")).unwrap();
        std::os::unix::fs::symlink(&device, drm.join("card8/device")).unwrap();
        fs::create_dir(drm.join("card8-HDMI-A-1")).unwrap();
        fs::write(drm.join("card8-HDMI-A-1/status"), "connected").unwrap();
    }
    #[test]
    fn deduplicates_clients_tracks_reparented_children_and_pid_reuse() {
        let dir = tempfile::tempdir().unwrap();
        let sys = dir.path().join("sys");
        let proc = dir.path().join("proc");
        fs::create_dir(&proc).unwrap();
        sys_fixture(&sys);
        let client = "drm-driver: amdgpu\ndrm-pdev: 0000:0e:00.0\ndrm-client-id: 7\ndrm-engine-gfx: 100 ns\ndrm-resident-vram: 4 KiB\n";
        process_fixture(&proc, 10, 1, 100, client);
        process_fixture(&proc, 11, 10, 200, client);
        let diagnostics = Diagnostics {
            session: "fixture".into(),
            kwin_support: None,
            display_configuration: None,
            versions: BTreeMap::new(),
            notes: vec![],
        };
        let mut collector = Collector::new(&sys, &proc, diagnostics);
        collector.launches.push(LaunchRegistration {
            process: ProcessId {
                pid: 10,
                start_ticks: 100,
            },
            gpu: "0000:0e:00.0".into(),
            profile: None,
            mangohud_directory: None,
            anchor_ns: 1,
        });
        let sample = collector.sample(&Config::default());
        assert_eq!(sample.coverage.clients_observed, 1);
        assert_eq!(sample.applications.len(), 1);
        assert_eq!(sample.applications[0].processes.len(), 2);
        assert_eq!(
            sample.applications[0].gpus["0000:0e:00.0"].memory["resident-vram"].value,
            Some(4096.0)
        );
        fs::remove_dir_all(proc.join("10")).unwrap();
        let stat = proc.join("11/stat");
        fs::write(
            &stat,
            fs::read_to_string(&stat)
                .unwrap()
                .replacen(" S 10 ", " S 1 ", 1),
        )
        .unwrap();
        let sample = collector.sample(&Config::default());
        assert_eq!(
            sample.applications[0].requested_gpu.as_deref(),
            Some("0000:0e:00.0")
        );
        // Reusing a PID with a different start-time cannot inherit the old profile.
        fs::remove_dir_all(proc.join("11")).unwrap();
        process_fixture(&proc, 11, 1, 300, client);
        let sample = collector.sample(&Config::default());
        assert!(sample.applications[0].requested_gpu.is_none());
        // Connector/device disappearance is rediscovered, not attached by cardN.
        fs::remove_dir_all(sys.join("class/drm/card8")).unwrap();
        assert!(collector.sample(&Config::default()).gpus.is_empty());
    }
    #[test]
    fn permission_error_is_reported_separately() {
        let mut coverage = Coverage::default();
        count_error(
            &mut coverage,
            &io::Error::from(io::ErrorKind::PermissionDenied),
        );
        assert_eq!(coverage.permission_denied, 1);
        assert_eq!(coverage.vanished, 0);
    }
    #[test]
    fn cached_clients_are_sampled_between_descriptor_discovery_passes() {
        let dir = tempfile::tempdir().unwrap();
        let sys = dir.path().join("sys");
        let proc = dir.path().join("proc");
        fs::create_dir(&proc).unwrap();
        sys_fixture(&sys);
        process_fixture(&proc, 10, 1, 100, "pos: 0\n");
        let diagnostics = Diagnostics {
            session: "fixture".into(),
            kwin_support: None,
            display_configuration: None,
            versions: BTreeMap::new(),
            notes: vec![],
        };
        let mut collector = Collector::new(&sys, &proc, diagnostics);
        assert!(collector.sample(&Config::default()).applications.is_empty());
        let client = "drm-driver: amdgpu\ndrm-pdev: 0000:0e:00.0\ndrm-client-id: 9\ndrm-engine-gfx: 100 ns\n";
        fs::write(proc.join("10/fdinfo/6"), client).unwrap();
        assert!(collector.sample(&Config::default()).applications.is_empty());
        // Age the discovery cache without a slow wall-clock sleep.
        for (time, _) in collector.gpu_fds.values_mut() {
            *time = 0;
        }
        assert_eq!(
            collector
                .sample(&Config::default())
                .coverage
                .clients_observed,
            1
        );
        fs::write(proc.join("10/fdinfo/6"), client.replace("100 ns", "200 ns")).unwrap();
        let sample = collector.sample(&Config::default());
        assert!(
            sample.applications[0].gpus["0000:0e:00.0"].engines["gfx"]
                .value
                .unwrap()
                > 0.0
        );
    }
}
