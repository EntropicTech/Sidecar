//! Candidate ranking and trial verification use measured, identity-bound history.
use crate::{config::Config, launchers::same_executable, model::*};
use serde::{Deserialize, Serialize};
use std::{borrow::Borrow, collections::VecDeque, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Management {
    Candidate,
    SavedProfile,
    ManagedElsewhere,
    Infrastructure,
    IgpuActive,
    Idle,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub process: ProcessId,
    pub name: String,
    pub executable: String,
    pub management: Management,
    pub profile: Option<String>,
    pub saved_target: Option<String>,
    pub graphics_percent: Option<f64>,
    pub video_percent: Option<f64>,
    pub other_percent: Option<f64>,
    pub observed_seconds: f64,
    pub active_seconds: f64,
    pub score: f64,
    pub observed_gpus: Vec<String>,
    pub source: String,
    #[serde(default)]
    pub launch_path: LaunchPath,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchKind {
    RegisteredProfile,
    RegisteredTrial,
    RegisteredCommand,
    Unregistered,
    ManagedElsewhere,
    Infrastructure,
    #[default]
    Unknown,
}
impl LaunchKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::RegisteredProfile => "Managed profile launch",
            Self::RegisteredTrial => "Sidecar test launch",
            Self::RegisteredCommand => "Sidecar command launch",
            Self::Unregistered => "Unregistered launch",
            Self::ManagedElsewhere => "Managed elsewhere",
            Self::Infrastructure => "Desktop infrastructure",
            Self::Unknown => "Launch origin unknown",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchPath {
    pub kind: LaunchKind,
    pub registration: Option<ProcessId>,
    pub registered_profile: Option<String>,
    pub matching_profiles: Vec<String>,
    pub explanation: String,
}
impl Default for LaunchPath {
    fn default() -> Self {
        Self {
            kind: LaunchKind::Unknown,
            registration: None,
            registered_profile: None,
            matching_profiles: vec![],
            explanation: "Launch-origin evidence is unavailable from this collector.".into(),
        }
    }
}

/// Registration establishes a Sidecar launch, never a specific desktop shortcut
/// or successful GPU placement. Unregistered does not establish a bypass.
pub fn launch_path(app: &Application, config: &Config) -> LaunchPath {
    let mut matching_profiles: Vec<_> = config
        .profiles
        .iter()
        .filter(|p| {
            (app.launch_id.is_some() && app.profile_name.as_ref() == Some(&p.name))
                || (!app.executable.is_empty() && same_executable(&p.executable, &app.executable))
        })
        .map(|p| p.name.clone())
        .collect();
    matching_profiles.sort();
    let (kind, explanation) = if config
        .managed_elsewhere
        .iter()
        .any(|p| !app.executable.is_empty() && same_executable(p, &app.executable))
    {
        (LaunchKind::ManagedElsewhere, "This executable is managed by another workflow. Sidecar does not recommend changing its launch settings.")
    } else if infrastructure(&app.executable) {
        (LaunchKind::Infrastructure, "Desktop infrastructure is observed separately and is not an application-routing candidate.")
    } else if app.launch_id.is_some() {
        if app.profile_name.as_ref().is_some_and(|name| {
            !name.starts_with("trial-") || config.profiles.iter().any(|p| &p.name == name)
        }) {
            (LaunchKind::RegisteredProfile, "This process group belongs to a registered Sidecar profile launch. GPU activity is verified separately; registration does not prove placement.")
        } else if app.profile_name.is_some() {
            (LaunchKind::RegisteredTrial, "This process group belongs to a registered unsaved Sidecar profile/test launch. Resume verification before Keep.")
        } else {
            (LaunchKind::RegisteredCommand, "This process group belongs to a registered Sidecar command launch, without a saved profile. GPU activity is verified separately.")
        }
    } else if app.executable.is_empty() {
        (LaunchKind::Unknown, "Executable identity is unavailable and no Sidecar registration is present. The launch origin cannot be established.")
    } else {
        (LaunchKind::Unregistered, "No launch registration exists in this collector. This does not identify a menu, pinned shortcut or session restore, or prove a profile was bypassed: a collector restart can lose registrations. For a controlled test, close the app normally and launch a selected profile; existing instances may reuse their original GPU settings.")
    };
    if matches!(
        kind,
        LaunchKind::ManagedElsewhere | LaunchKind::Infrastructure
    ) {
        matching_profiles.clear();
    }
    LaunchPath {
        kind,
        registration: app.launch_id.clone(),
        registered_profile: app.launch_id.as_ref().and(app.profile_name.clone()),
        matching_profiles,
        explanation: explanation.into(),
    }
}

pub fn infrastructure(executable: &str) -> bool {
    let executable = executable.strip_suffix(" (deleted)").unwrap_or(executable);
    matches!(
        Path::new(executable)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(""),
        "kwin_wayland"
            | "kwin_x11"
            | "Xwayland"
            | "Xorg"
            | "plasmashell"
            | "igpu-mand"
            | "igpu-man-desktop"
    )
}

fn readable(metric: &Metric) -> Option<f64> {
    if !matches!(metric.status, Availability::Available | Availability::Idle) {
        return None;
    }
    metric.value.filter(|v| v.is_finite() && *v >= 0.0)
}
fn rendering(engine: &str) -> bool {
    matches!(engine, "gfx" | "graphics" | "render" | "3d")
}
fn decoding(engine: &str) -> bool {
    engine == "dec" || engine.contains("decode")
}
fn video(engine: &str) -> bool {
    decoding(engine)
        || engine == "enc"
        || engine.contains("video")
        || engine.contains("vcn")
        || engine.contains("encode")
}

pub fn candidates<S: Borrow<Snapshot>>(
    snapshot: &Snapshot,
    history: &VecDeque<S>,
    config: &Config,
) -> Vec<Candidate> {
    let mut result = Vec::new();
    for app in &snapshot.applications {
        let mut weighted = [0.0; 3];
        let mut durations = [0.0; 3];
        let mut observed_seconds = 0.0;
        let mut active_seconds = 0.0;
        let mut previous = None;
        for sample in history.iter().map(Borrow::borrow).filter(|s| {
            s.monotonic_ns <= snapshot.monotonic_ns
                && snapshot.monotonic_ns - s.monotonic_ns <= 60_000_000_000
        }) {
            let old = previous.replace(sample.monotonic_ns);
            let Some(old) = old else {
                continue;
            };
            // A gap (suspend, reconnect) must not invent sustained activity.
            let delta = sample.monotonic_ns.saturating_sub(old);
            if delta == 0 || delta > 3_000_000_000 {
                continue;
            }
            let seconds = delta as f64 / 1e9;
            let Some(current) = sample.applications.iter().find(|a| a.id == app.id) else {
                continue;
            };
            let mut values: [Option<f64>; 3] = [None; 3];
            for gpu in sample.gpus.iter().filter(|g| !g.integrated) {
                if let Some(usage) = current.gpus.get(&gpu.pci) {
                    for (engine, metric) in &usage.engines {
                        if let Some(value) = readable(metric) {
                            let bucket = if rendering(engine) {
                                0
                            } else if video(engine) {
                                1
                            } else {
                                2
                            };
                            *values[bucket].get_or_insert(0.0) += value;
                        }
                    }
                }
            }
            if values.iter().any(Option::is_some) {
                observed_seconds += seconds;
            }
            if values.iter().flatten().any(|v| *v > 0.0) {
                active_seconds += seconds;
            }
            for i in 0..3 {
                if let Some(value) = values[i] {
                    weighted[i] += value * seconds;
                    durations[i] += seconds;
                }
            }
        }
        let mean: [Option<f64>; 3] =
            std::array::from_fn(|i| (durations[i] > 0.0).then(|| weighted[i] / durations[i]));
        let score = mean.iter().flatten().sum::<f64>();
        let path = launch_path(app, config);
        let registered = app
            .launch_id
            .as_ref()
            .and(app.profile_name.as_ref())
            .and_then(|name| config.profiles.iter().find(|p| &p.name == name));
        let matching: Vec<_> = config
            .profiles
            .iter()
            .filter(|p| path.matching_profiles.contains(&p.name))
            .collect();
        let profile = registered
            .map(|p| p.name.clone())
            .or_else(|| (matching.len() == 1).then(|| matching[0].name.clone()));
        // Multiple saved targets are alternatives, not evidence of launch intent.
        let saved_target = if app.launch_id.is_some() {
            app.requested_gpu.clone()
        } else {
            matching
                .first()
                .filter(|first| matching.iter().all(|p| p.gpu == first.gpu))
                .map(|p| p.gpu.clone())
        };
        let management = if config
            .managed_elsewhere
            .iter()
            .any(|p| same_executable(p, &app.executable))
        {
            Management::ManagedElsewhere
        } else if infrastructure(&app.executable) {
            Management::Infrastructure
        } else if registered.is_some() || !matching.is_empty() {
            Management::SavedProfile
        } else if score > 0.0 {
            Management::Candidate
        } else if snapshot
            .gpus
            .iter()
            .filter(|g| g.integrated)
            .any(|g| app.observed_gpus.contains(&g.pci))
        {
            Management::IgpuActive
        } else if observed_seconds > 0.0 {
            Management::Idle
        } else {
            Management::Unavailable
        };
        result.push(Candidate { process: app.id.clone(), name: app.name.clone(), executable: app.executable.clone(), management, profile, saved_target,
            graphics_percent: mean[0], video_percent: mean[1], other_percent: mean[2], observed_seconds, active_seconds, score,
            observed_gpus: app.observed_gpus.clone(), launch_path: path, source: "Accessible DRM clients via /proc/fdinfo; time-weighted last 60 seconds. Engine times are not whole-device GPU shares or predicted FPS gains.".into() });
    }
    result.sort_by(|a, b| {
        let rank = |c: &Candidate| {
            if c.management == Management::Candidate {
                0
            } else if c.management == Management::SavedProfile {
                1
            } else {
                2
            }
        };
        rank(a)
            .cmp(&rank(b))
            .then_with(|| b.score.total_cmp(&a.score))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.process.cmp(&b.process))
    });
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    Waiting,
    TargetGpu,
    OtherGpu,
    BothGpus,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Verification {
    pub application_seen: bool,
    pub rendering: Placement,
    pub decoding: Placement,
    pub other_engines: Placement,
    pub can_keep: bool,
    pub evidence_seconds: f64,
    pub explanation: String,
}

#[derive(Default)]
struct Evidence {
    target: Vec<u64>,
    other: Vec<u64>,
    readable: bool,
}
impl Evidence {
    fn state(&self) -> Placement {
        let sustained = |times: &[u64]| {
            times.len() >= 3 && times.last().unwrap() - times.first().unwrap() >= 2_000_000_000
        };
        match (sustained(&self.target), !self.other.is_empty()) {
            (true, true) => Placement::BothGpus,
            (true, false) => Placement::TargetGpu,
            (false, true) => Placement::OtherGpu,
            _ if self.readable => Placement::Waiting,
            _ => Placement::Unavailable,
        }
    }
}

pub fn verify_trial<S: Borrow<Snapshot>>(
    snapshot: &Snapshot,
    history: &VecDeque<S>,
    process: &ProcessId,
    gpu: &str,
    anchor_ns: u64,
) -> Verification {
    let matches = |a: &Application| a.launch_id.as_ref() == Some(process) || &a.id == process;
    let application_seen = snapshot.applications.iter().any(matches);
    let mut evidence: [Evidence; 3] = std::array::from_fn(|_| Evidence::default());
    let mut last = None;
    let mut first = None;
    for sample in history.iter().map(Borrow::borrow).filter(|s| {
        s.monotonic_ns >= anchor_ns
            && s.monotonic_ns <= snapshot.monotonic_ns
            && snapshot.monotonic_ns - s.monotonic_ns <= 15_000_000_000
    }) {
        if last.is_some_and(|t| sample.monotonic_ns <= t) {
            continue;
        }
        // Resume/reconnect starts a fresh verification window.
        if last.is_some_and(|t| sample.monotonic_ns - t > 3_000_000_000) {
            evidence = std::array::from_fn(|_| Evidence::default());
            first = None;
        }
        last = Some(sample.monotonic_ns);
        let Some(app) = sample.applications.iter().find(|a| matches(a)) else {
            continue;
        };
        first.get_or_insert(sample.monotonic_ns);
        let mut active = [[false; 2]; 3];
        for (pci, usage) in &app.gpus {
            for (engine, metric) in &usage.engines {
                let bucket = if rendering(engine) {
                    0
                } else if decoding(engine) {
                    1
                } else {
                    2
                };
                if let Some(value) = readable(metric) {
                    evidence[bucket].readable = true;
                    if value > 0.0 {
                        active[bucket][usize::from(pci != gpu)] = true;
                    }
                }
            }
        }
        for i in 0..3 {
            if active[i][0] {
                evidence[i].target.push(sample.monotonic_ns);
            }
            if active[i][1] {
                evidence[i].other.push(sample.monotonic_ns);
            }
        }
    }
    let mut states = evidence.map(|e| e.state());
    let mut current_readable = [false; 3];
    if let Some(app) = snapshot.applications.iter().find(|a| matches(a)) {
        for (_, usage) in app.gpus.iter().filter(|(pci, _)| pci.as_str() == gpu) {
            for (engine, metric) in &usage.engines {
                if readable(metric).is_some() {
                    let bucket = if rendering(engine) {
                        0
                    } else if decoding(engine) {
                        1
                    } else {
                        2
                    };
                    current_readable[bucket] = true;
                }
            }
        }
    }
    for i in 0..3 {
        if states[i] == Placement::TargetGpu && !current_readable[i] {
            states[i] = Placement::Unavailable;
        }
    }
    let target = states.contains(&Placement::TargetGpu);
    let elsewhere = states
        .iter()
        .any(|s| matches!(s, Placement::OtherGpu | Placement::BothGpus));
    let can_keep = application_seen && target && !elsewhere;
    let explanation = if !application_seen {
        "Waiting for the new launch. If it exits or reuses an existing instance, close that instance normally and retry."
    } else if elsewhere {
        "Activity measured on another GPU. Inspect rendering and decoding separately before keeping this profile."
    } else if can_keep {
        "Sustained iGPU activity verified across at least three samples spanning two seconds. Decoder verification requires active video playback."
    } else {
        "Use the application to generate activity. A device handle, startup probe, or missing decoder counter is not sufficient verification."
    };
    Verification {
        application_seen,
        rendering: states[0],
        decoding: states[1],
        other_engines: states[2],
        can_keep,
        evidence_seconds: first
            .map(|t| (snapshot.monotonic_ns - t) as f64 / 1e9)
            .unwrap_or(0.0),
        explanation: explanation.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[test]
    fn launch_audit_separates_registration_matching_profiles_and_external_management() {
        let mut snapshot = sample(1, 10, 20, "dgpu", 5.);
        let mut config = Config::default();
        for (name, gpu) in [("app-dgpu", "dgpu"), ("app-igpu", "igpu")] {
            config.profiles.push(Profile {
                name: name.into(),
                executable: "/tmp/app".into(),
                gpu: gpu.into(),
                ..Default::default()
            });
        }
        let app = &mut snapshot.applications[0];
        app.launch_id = None;
        let path = launch_path(app, &config);
        assert_eq!(path.kind, LaunchKind::Unregistered);
        assert_eq!(path.matching_profiles, ["app-dgpu", "app-igpu"]);
        let candidate = &candidates(&snapshot, &VecDeque::<Snapshot>::new(), &config)[0];
        assert!(candidate.saved_target.is_none());
        assert!(candidate.profile.is_none());
        assert_eq!(candidate.management, Management::SavedProfile);
        let app = &mut snapshot.applications[0];
        app.launch_id = Some(ProcessId {
            pid: 9,
            start_ticks: 19,
        }); // registered wrapper exited
        app.profile_name = Some("app-igpu".into());
        app.requested_gpu = Some("igpu".into());
        let path = launch_path(app, &config);
        assert_eq!(path.kind, LaunchKind::RegisteredProfile);
        assert_eq!(path.registration.as_ref().unwrap().pid, 9);
        let candidate = &candidates(&snapshot, &VecDeque::<Snapshot>::new(), &config)[0];
        assert_eq!(candidate.profile.as_deref(), Some("app-igpu"));
        assert_eq!(candidate.saved_target.as_deref(), Some("igpu"));
        let app = &mut snapshot.applications[0];
        app.profile_name = Some("trial-test".into());
        assert_eq!(launch_path(app, &config).kind, LaunchKind::RegisteredTrial);
        app.profile_name = None;
        assert_eq!(
            launch_path(app, &config).kind,
            LaunchKind::RegisteredCommand
        );
        config.managed_elsewhere.insert(app.executable.clone());
        let path = launch_path(app, &config);
        assert_eq!(path.kind, LaunchKind::ManagedElsewhere);
        assert!(path.matching_profiles.is_empty());
        app.launch_id = None;
        app.executable.clear();
        assert_eq!(launch_path(app, &config).kind, LaunchKind::Unknown);
        app.executable = "/usr/bin/kwin_wayland".into();
        assert_eq!(launch_path(app, &config).kind, LaunchKind::Infrastructure);
    }
    #[test]
    fn older_candidate_responses_have_unknown_launch_evidence() {
        let snapshot = sample(1, 10, 20, "dgpu", 0.);
        let candidate =
            candidates(&snapshot, &VecDeque::<Snapshot>::new(), &Config::default()).remove(0);
        let mut value = serde_json::to_value(candidate).unwrap();
        value.as_object_mut().unwrap().remove("launch_path");
        let loaded: Candidate = serde_json::from_value(value).unwrap();
        assert_eq!(loaded.launch_path.kind, LaunchKind::Unknown);
    }
    fn sample(time: u64, pid: u32, start: u64, pci: &str, value: f64) -> Snapshot {
        let id = ProcessId {
            pid,
            start_ticks: start,
        };
        Snapshot {
            api_version: 1,
            monotonic_ns: time * 1_000_000_000,
            unix_ms: 0,
            gpus: vec![Gpu {
                pci: "dgpu".into(),
                name: "dGPU".into(),
                alias: "dgpu".into(),
                integrated: false,
                card: "".into(),
                render_node: None,
                capabilities: vec![],
                outputs: vec![],
                metrics: BTreeMap::new(),
            }],
            applications: vec![Application {
                id: id.clone(),
                launch_id: Some(id),
                profile_name: None,
                name: "App".into(),
                executable: "/tmp/app".into(),
                processes: vec![],
                requested_gpu: None,
                observed_gpus: vec![],
                placement: "unverified".into(),
                cpu_percent: Metric::measured(0., "%", "fixture"),
                gpus: BTreeMap::from([(
                    pci.into(),
                    GpuUsage {
                        opened: true,
                        engines: BTreeMap::from([(
                            "gfx".into(),
                            Metric::measured(value, "%", "fixture"),
                        )]),
                        memory: BTreeMap::new(),
                    },
                )]),
            }],
            coverage: Coverage::default(),
            diagnostics: Diagnostics {
                session: "test".into(),
                kwin_support: None,
                display_configuration: None,
                versions: BTreeMap::new(),
                notes: vec![],
            },
            recommendations: vec![],
            active_recording: None,
            collector_duration_ms: 0.,
        }
    }
    #[test]
    fn ranking_weights_time_and_persistently_excludes_managed_elsewhere() {
        let history = VecDeque::from([
            sample(1, 1, 1, "dgpu", 10.),
            sample(2, 1, 1, "dgpu", 2.),
            sample(3, 1, 1, "dgpu", 4.),
        ]);
        let mut config = Config::default();
        let c = &candidates(history.back().unwrap(), &history, &config)[0];
        assert_eq!(c.graphics_percent, Some(3.));
        assert_eq!(c.observed_seconds, 2.);
        config.managed_elsewhere.insert("/tmp/app".into());
        assert_eq!(
            candidates(history.back().unwrap(), &history, &config)[0].management,
            Management::ManagedElsewhere
        );
        let loaded: Config = toml::from_str(&toml::to_string(&config).unwrap()).unwrap();
        assert!(loaded.managed_elsewhere.contains("/tmp/app"));
    }
    #[test]
    fn no_activity_handles_and_stale_metrics_are_not_candidates() {
        assert!(infrastructure("/tmp/igpu-man-desktop (deleted)"));
        let mut history =
            VecDeque::from([sample(1, 1, 1, "dgpu", 0.), sample(2, 1, 1, "dgpu", 0.)]);
        assert_eq!(
            candidates(history.back().unwrap(), &history, &Config::default())[0].management,
            Management::Idle
        );
        for s in &mut history {
            s.applications[0]
                .gpus
                .get_mut("dgpu")
                .unwrap()
                .engines
                .get_mut("gfx")
                .unwrap()
                .status = Availability::Stale;
        }
        assert_eq!(
            candidates(history.back().unwrap(), &history, &Config::default())[0].management,
            Management::Unavailable
        );
        for s in &mut history {
            s.applications[0]
                .gpus
                .get_mut("dgpu")
                .unwrap()
                .engines
                .clear();
        }
        assert_eq!(
            candidates(history.back().unwrap(), &history, &Config::default())[0].management,
            Management::Unavailable
        );
    }
    #[test]
    fn trial_requires_sustained_activity_and_never_adopts_a_reused_pid() {
        let id = ProcessId {
            pid: 1,
            start_ticks: 1,
        };
        let mut history = VecDeque::from([
            sample(1, 1, 1, "igpu", 10.),
            sample(2, 1, 1, "igpu", 0.),
            sample(3, 1, 1, "igpu", 0.),
        ]);
        assert!(!verify_trial(history.back().unwrap(), &history, &id, "igpu", 0).can_keep);
        history[1] = sample(2, 1, 1, "igpu", 5.);
        history[2] = sample(3, 1, 1, "igpu", 5.);
        let result = verify_trial(history.back().unwrap(), &history, &id, "igpu", 0);
        assert!(result.can_keep);
        assert_eq!(result.decoding, Placement::Unavailable);
        history[2] = sample(3, 1, 2, "igpu", 5.);
        assert!(!verify_trial(history.back().unwrap(), &history, &id, "igpu", 0).can_keep);
    }
    #[test]
    fn both_gpus_and_permission_failures_prevent_false_success() {
        let id = ProcessId {
            pid: 1,
            start_ticks: 1,
        };
        let mut history: VecDeque<_> = (1..=3).map(|t| sample(t, 1, 1, "igpu", 5.)).collect();
        history[2].applications[0].gpus.insert(
            "dgpu".into(),
            sample(3, 1, 1, "dgpu", 1.)
                .applications
                .remove(0)
                .gpus
                .remove("dgpu")
                .unwrap(),
        );
        let result = verify_trial(history.back().unwrap(), &history, &id, "igpu", 0);
        assert_eq!(result.rendering, Placement::BothGpus);
        assert!(!result.can_keep);
        for s in &mut history {
            for u in s.applications[0].gpus.values_mut() {
                for m in u.engines.values_mut() {
                    m.status = Availability::PermissionDenied;
                }
            }
        }
        let result = verify_trial(history.back().unwrap(), &history, &id, "igpu", 0);
        assert_eq!(result.rendering, Placement::Unavailable);
        assert!(!result.can_keep);
    }
    #[test]
    fn suspend_gap_requires_new_verification() {
        let id = ProcessId {
            pid: 1,
            start_ticks: 1,
        };
        let history = VecDeque::from([
            sample(1, 1, 1, "igpu", 5.),
            sample(2, 1, 1, "igpu", 5.),
            sample(3, 1, 1, "igpu", 5.),
            sample(12, 1, 1, "igpu", 5.),
        ]);
        assert!(!verify_trial(history.back().unwrap(), &history, &id, "igpu", 0).can_keep);
    }
    #[test]
    fn sustained_work_ranks_ahead_of_a_brief_spike() {
        let history: VecDeque<_> = (1..=61)
            .map(|time| {
                let mut s = sample(time, 1, 1, "dgpu", if time == 2 { 10. } else { 0. });
                let mut steady = sample(time, 2, 1, "dgpu", 1.).applications.remove(0);
                steady.executable = "/tmp/steady".into();
                s.applications.push(steady);
                s
            })
            .collect();
        let rows = candidates(history.back().unwrap(), &history, &Config::default());
        assert_eq!(rows[0].process.pid, 2);
        assert_eq!(rows[0].observed_seconds, 60.);
        assert_eq!(rows[1].active_seconds, 1.);
    }
    #[test]
    fn permission_loss_at_the_latest_sample_invalidates_keep() {
        let id = ProcessId {
            pid: 1,
            start_ticks: 1,
        };
        let mut history: VecDeque<_> = (1..=4).map(|t| sample(t, 1, 1, "igpu", 5.)).collect();
        history[3].applications[0]
            .gpus
            .get_mut("igpu")
            .unwrap()
            .engines
            .get_mut("gfx")
            .unwrap()
            .status = Availability::PermissionDenied;
        let v = verify_trial(history.back().unwrap(), &history, &id, "igpu", 0);
        assert_eq!(v.rendering, Placement::Unavailable);
        assert!(!v.can_keep);
    }
    #[test]
    fn reparented_descendants_keep_the_launch_identity_without_mislabeling_enc_as_decode() {
        let id = ProcessId {
            pid: 1,
            start_ticks: 1,
        };
        let history: VecDeque<_> = (1..=3)
            .map(|t| {
                let mut s = sample(t, 2, 2, "igpu", 5.);
                s.applications[0].launch_id = Some(id.clone());
                let usage = s.applications[0].gpus.get_mut("igpu").unwrap();
                let metric = usage.engines.remove("gfx").unwrap();
                usage.engines.insert("enc".into(), metric);
                s
            })
            .collect();
        let v = verify_trial(history.back().unwrap(), &history, &id, "igpu", 0);
        assert!(v.can_keep);
        assert_eq!(v.decoding, Placement::Unavailable);
        assert_eq!(v.other_engines, Placement::TargetGpu);
    }
}
