use igpu_core::api::Operation;
mod dispatch;
mod live;
mod worker;
use anyhow::{bail, Context, Result};
use igpu_core::{
    applications,
    collector::{self, Collector},
    config::{Config, Paths},
    launch, launchers,
    model::*,
    recording,
    storage::Store,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeSet, VecDeque},
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

fn validate_update_token(token: &str) -> Result<()> {
    anyhow::ensure!(
        token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit()),
        "Update token must contain 64 hexadecimal characters"
    );
    Ok(())
}
fn boot_update_lease() -> Result<Option<(String, u64)>> {
    match std::env::var("IGPU_MAN_UPDATE_TOKEN") {
        Ok(token) => {
            validate_update_token(&token)?;
            Ok(Some((token, monotonic_ns() + 300_000_000_000)))
        }
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(_) => bail!("Invalid update-token encoding"),
    }
}

struct State {
    live: Arc<Mutex<live::Live>>,
    config: Config,
    paths: Paths,
    store: Store,
    active: Option<i64>,
    deadline: Option<u64>,
    last_saved_ns: u64,
    writer_delayed: bool,
    health: Vec<String>,
    imported: BTreeSet<PathBuf>,
    frame_anchors: std::collections::BTreeMap<PathBuf, (i64, u64, f64)>,
    managed_directories: std::collections::BTreeMap<PathBuf, i64>,
}

impl State {
    fn start(&mut self, label: &str, include_buffer: bool) -> Result<i64> {
        if self.active.is_some() {
            bail!("A recording is already active");
        }
        let now = monotonic_ns();
        let view = self
            .live
            .lock()
            .map_err(|_| anyhow::anyhow!("Live state poisoned"))?
            .view();
        let started = if include_buffer {
            view.ring.front().map(|s| s.monotonic_ns).unwrap_or(now)
        } else {
            now
        };
        let snapshot = &view.snapshot;
        let environment = json!({"diagnostics":snapshot.diagnostics,"gpus":snapshot.gpus.iter().map(|g|json!({"pci":g.pci,"name":g.name,"outputs":g.outputs})).collect::<Vec<_>>()});
        let metadata = json!({"schema_version":1,"igpu_man_build":igpu_core::build::metadata(),"boot_id":fs::read_to_string("/proc/sys/kernel/random/boot_id").unwrap_or_default().trim(),"environment":environment,"profiles":self.config.profiles,"launches":view.launches,"normal_interval_ms":self.config.sample_interval_ms,"recording_interval_ms":self.config.recording_interval_ms,"stutter_threshold_ms":self.config.stutter_threshold_ms,"note":"Frame logs require alignment metadata; telemetry cannot resolve individual frame stalls."});
        let id = self.store.start(label, started, &metadata)?;
        let result = (|| -> Result<()> {
            if include_buffer {
                for sample in &view.ring {
                    self.store.sample(id, sample)?;
                }
            }
            let mut live = self
                .live
                .lock()
                .map_err(|_| anyhow::anyhow!("Live state poisoned"))?;
            let config = live.config.clone();
            let mut sample = live.collector.sample(&config);
            sample.active_recording = Some(id);
            let sample = Arc::new(sample);
            live.snapshot = sample.clone();
            self.last_saved_ns = sample.monotonic_ns;
            // Initial persistence happens outside the sampler lock. The writer
            // owns start/stop ordering, while the sampler can already publish.
            live.active = Some(id);
            self.active = Some(id);
            self.deadline = None;
            live.wake.notify_one();
            drop(live);
            self.store.sample(id, &sample)?;
            Ok(())
        })();
        if let Err(error) = result {
            igpu_core::logging::event(
                "error",
                "recording.start_failed",
                igpu_core::logging::error_fields(&error),
            );
            if let Err(cleanup) = self.store.stop(id, monotonic_ns(), "storage_error") {
                igpu_core::logging::event(
                    "error",
                    "recording.stop_persist_failed",
                    igpu_core::logging::error_fields(&cleanup),
                );
            }
            self.active = None;
            let mut live = self
                .live
                .lock()
                .map_err(|_| anyhow::anyhow!("Live state poisoned"))?;
            live.active = None;
            Arc::make_mut(&mut live.snapshot).active_recording = None;
            return Err(error);
        }
        igpu_core::logging::event(
            "info",
            "recording.started",
            json!({"recording_id":id,"included_buffer":include_buffer}),
        );
        Ok(id)
    }
    fn stop(&mut self, reason: &str) -> Result<i64> {
        let id = self.active.context("No recording is active")?;
        {
            let mut live = self
                .live
                .lock()
                .map_err(|_| anyhow::anyhow!("Live state poisoned"))?;
            live.active = None;
            live.stopping = Some(id);
            live.wake.notify_one();
        }
        self.observe_frame_logs()?;
        self.import_managed_logs(id);
        let result = self.store.stop(id, monotonic_ns(), reason);
        self.active = None;
        self.deadline = None;
        {
            let mut live = self
                .live
                .lock()
                .map_err(|_| anyhow::anyhow!("Live state poisoned"))?;
            live.stopping = None;
            Arc::make_mut(&mut live.snapshot).active_recording = None;
        }
        result?;
        igpu_core::logging::event(
            "info",
            "recording.stopped",
            json!({"recording_id":id,"reason":reason}),
        );
        Ok(id)
    }
    fn marker(&mut self, label: &str) -> Result<Value> {
        if label.len() > 1024 {
            bail!("Marker label exceeds 1024 bytes");
        }
        if self.active.is_none() {
            self.start("stutter-capture", true)?;
            self.deadline = Some(monotonic_ns() + 30_000_000_000);
        } else if self.deadline.is_some() {
            self.deadline = Some(monotonic_ns() + 30_000_000_000);
        }
        let id = self.active.context("Marker has no active recording")?;
        self.store.marker(
            id,
            &Marker {
                monotonic_ns: monotonic_ns(),
                label: label.into(),
            },
        )?;
        Ok(json!({"recording_id": id, "capture_until_ns": self.deadline}))
    }
    fn observe_frame_logs(&mut self) -> Result<()> {
        let collection_ms = self
            .live
            .lock()
            .map_err(|_| anyhow::anyhow!("Live state poisoned"))?
            .snapshot
            .collector_duration_ms;
        for (directory, recording_id) in &self.managed_directories {
            if Some(*recording_id) != self.active {
                continue;
            }
            let Ok(entries) = fs::read_dir(directory) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|e| e != "csv")
                    || path.to_string_lossy().ends_with("_summary.csv")
                    || self.frame_anchors.contains_key(&path)
                {
                    continue;
                }
                let Ok(raw) = igpu_core::fsutil::read_text_limited(&path, 64 * 1024 * 1024) else {
                    continue;
                };
                // File first-observation bounds the logging-start estimate. It is
                // never presented as exact frame-to-telemetry synchronization.
                if let Ok(log) = recording::parse_frame_log(0, String::new(), raw, None, None) {
                    if let Some(frame) = log.frames.last() {
                        self.frame_anchors.insert(
                            path,
                            (
                                *recording_id,
                                monotonic_ns().saturating_sub(frame.elapsed_ns),
                                self.config.recording_interval_ms as f64
                                    + collection_ms
                                    + monotonic_ns().saturating_sub(self.last_saved_ns) as f64
                                        / 1e6,
                            ),
                        );
                    }
                }
            }
        }
        Ok(())
    }
    fn import_managed_logs(&mut self, id: i64) {
        let logs: Vec<_> = self
            .frame_anchors
            .iter()
            .map(|(p, a)| (p.clone(), *a))
            .collect();
        for (path, (recording_id, anchor, uncertainty)) in logs {
            if recording_id != id {
                continue;
            }
            if self.imported.contains(&path) {
                continue;
            }
            let result = (|| -> Result<()> {
                let metadata = fs::metadata(&path)?;
                if metadata.len() > 64 * 1024 * 1024 {
                    bail!("Managed frame log too large");
                }
                let raw = igpu_core::fsutil::read_text_limited(&path, 64 * 1024 * 1024)?;
                let log = recording::parse_frame_log(
                    id,
                    path.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    raw,
                    Some(anchor),
                    Some(uncertainty),
                )?;
                self.store.import(&log)?;
                Ok(())
            })();
            match result {
                Ok(()) => {
                    self.imported.insert(path);
                }
                Err(e) => self.health.push(format!("Managed MangoHud import: {e:#}")),
            }
        }
        for (directory, recording_id) in &self.managed_directories {
            if *recording_id == id && !self.imported.iter().any(|path| path.starts_with(directory))
            {
                self.health.push("No compatible MangoHud frame log was attached for a managed launch. Confirm MangoHud support or import its log manually.".into());
            }
        }
    }
    fn persist_sample(&mut self, sample: &Snapshot) -> Result<()> {
        let delayed = monotonic_ns().saturating_sub(sample.monotonic_ns) > 2_000_000_000;
        if delayed != self.writer_delayed {
            igpu_core::logging::event(
                if delayed { "warn" } else { "info" },
                if delayed {
                    "recording.writer_delayed"
                } else {
                    "recording.writer_recovered"
                },
                json!({"delay_ms":monotonic_ns().saturating_sub(sample.monotonic_ns)/1_000_000}),
            );
            self.writer_delayed = delayed;
            self.health.retain(|s|s!="Recording writes are delayed; samples are buffered while live collection continues.");
            if delayed {
                self.health.push("Recording writes are delayed; samples are buffered while live collection continues.".into());
            }
        }
        if let Some(id) = sample
            .active_recording
            .filter(|id| Some(*id) == self.active)
        {
            if sample.monotonic_ns > self.last_saved_ns {
                if let Err(error) = self.store.sample(id, sample) {
                    igpu_core::logging::event(
                        "error",
                        "recording.sample_failed",
                        igpu_core::logging::error_fields(&error),
                    );
                    self.health.push(
                        "Recording stopped because persistence failed; inspect collector logs."
                            .into(),
                    );
                    self.stop("storage_error")?;
                }
                self.last_saved_ns = sample.monotonic_ns;
            }
            if self.active.is_some() {
                self.observe_frame_logs()?;
            }
            if self.active.is_some()
                && self
                    .deadline
                    .is_some_and(|deadline| sample.monotonic_ns >= deadline)
            {
                self.stop("complete")?;
            }
        }
        self.health.truncate(20);
        self.live
            .lock()
            .map_err(|_| anyhow::anyhow!("Live state poisoned"))?
            .health = self.health.clone();
        Ok(())
    }
    fn request(&mut self, method: Operation, mut value: Value) -> Result<Value> {
        self.live
            .lock()
            .map_err(|_| anyhow::anyhow!("Live state poisoned"))?
            .check_update(method, value["_update_token"].as_str())?;
        if let Some(object) = value.as_object_mut() {
            object.remove("_update_token");
        }
        let view = self
            .live
            .lock()
            .map_err(|_| anyhow::anyhow!("Live state poisoned"))?
            .view();
        let snapshot = &view.snapshot;
        let ring = &view.ring;
        let id = || value["id"].as_i64().context("Missing recording id");
        let text = |key: &str| {
            value[key]
                .as_str()
                .with_context(|| format!("Missing {key}"))
        };
        match method {
            Operation::PrepareUpdate => {
                let token = value["token"].as_str().context("Missing update token")?;
                validate_update_token(token)?;
                anyhow::ensure!(
                    self.active.is_none(),
                    "Stop your recording before updating; nothing was changed"
                );
                let mut live = self
                    .live
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Live state poisoned"))?;
                anyhow::ensure!(
                    live.active.is_none() && live.stopping.is_none(),
                    "A recording is active or stopping"
                );
                anyhow::ensure!(
                    live.update_lease
                        .as_ref()
                        .is_none_or(|(held, _)| held == token),
                    "Another updater holds the maintenance lease"
                );
                let until = monotonic_ns() + 300_000_000_000;
                live.update_lease = Some((token.into(), until));
                let launches = live.collector.launches.clone();
                drop(live);
                Ok(
                    json!({"launches":launches,"lease_until_ns":until,"storage_schema":1,"config_schema":1}),
                )
            }
            Operation::CancelUpdate => {
                let token = value["token"].as_str().context("Missing update token")?;
                let mut live = self
                    .live
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Live state poisoned"))?;
                anyhow::ensure!(
                    live.update_lease
                        .as_ref()
                        .is_none_or(|(held, _)| held == token),
                    "Update lease token mismatch"
                );
                live.update_lease = None;
                Ok(json!(true))
            }
            Operation::Capabilities => Ok(
                json!({"api_version":igpu_core::API_VERSION,"features":["guarded_stop","request_ids","pending_trials","structured_logging"]}),
            ),
            Operation::PendingTrials => Ok(serde_json::to_value(
                self.live
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Live state poisoned"))?
                    .collector
                    .launches
                    .iter()
                    .filter(|launch| {
                        launch.profile.as_ref().is_some_and(|p| {
                            p.name.starts_with("trial-")
                                && !self
                                    .config
                                    .profiles
                                    .iter()
                                    .any(|saved| saved.name == p.name)
                        })
                    })
                    .collect::<Vec<_>>(),
            )?),
            Operation::DiscardTrial => {
                let process: ProcessId = serde_json::from_value(value["process"].clone())?;
                let mut live = self
                    .live
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Live state poisoned"))?;
                let launch = live
                    .collector
                    .launches
                    .iter_mut()
                    .find(|l| l.process == process)
                    .context("Trial is no longer tracked")?;
                if !launch.profile.as_ref().is_some_and(|p| {
                    p.name.starts_with("trial-")
                        && !self
                            .config
                            .profiles
                            .iter()
                            .any(|saved| saved.name == p.name)
                }) {
                    bail!("This is not an unkept trial");
                }
                launch.profile = None;
                Ok(json!(true))
            }
            Operation::Snapshot => Ok(serde_json::to_value(snapshot)?),
            Operation::Candidates => Ok(serde_json::to_value(applications::candidates(
                snapshot,
                ring,
                &self.config,
            ))?),
            Operation::Launchers => Ok(serde_json::to_value(launchers::installed())?),
            Operation::Management => Ok(serde_json::to_value(&self.config.managed_elsewhere)?),
            Operation::SetManagement => {
                let executable = text("executable")?;
                if !std::path::Path::new(executable).is_absolute() || executable.contains('\0') {
                    bail!("Expected an absolute executable identity");
                }
                let mut next = self.config.clone();
                next.managed_elsewhere
                    .retain(|p| !launchers::same_executable(p, executable));
                if value["managed_elsewhere"]
                    .as_bool()
                    .context("Missing managed_elsewhere boolean")?
                {
                    next.managed_elsewhere.insert(
                        launchers::executable_path(executable)
                            .map(|p| p.to_string_lossy().into_owned())
                            .unwrap_or_else(|| executable.into()),
                    );
                }
                next.save(&self.paths)?;
                self.config = next;
                Ok(serde_json::to_value(&self.config.managed_elsewhere)?)
            }
            Operation::VerifyTrial | Operation::KeepTrial => {
                let process: ProcessId = serde_json::from_value(value["process"].clone())?;
                let registration = view.launches.iter().find(|l| l.process == process).cloned();
                let Some(registration) = registration else {
                    if method == Operation::KeepTrial {
                        bail!("Trial launch is no longer tracked. Relaunch and verify before keeping it.");
                    }
                    return Ok(serde_json::to_value(applications::Verification {
                        application_seen: false, rendering: applications::Placement::Waiting,
                        decoding: applications::Placement::Waiting, other_engines: applications::Placement::Waiting,
                        can_keep: false, evidence_seconds: 0.0,
                        explanation: "Waiting for this launch to register. If it exited, reused another instance, or the collector restarted, discard this setup and retry.".into(),
                    })?);
                };
                let verification = applications::verify_trial(
                    snapshot,
                    ring,
                    &process,
                    &registration.gpu,
                    registration.anchor_ns,
                );
                if method == Operation::VerifyTrial {
                    return Ok(serde_json::to_value(verification)?);
                }
                if monotonic_ns().saturating_sub(snapshot.monotonic_ns) > 3_000_000_000
                    || !verification.can_keep
                {
                    bail!("Sustained target-GPU activity must be verified on a live, fresh trial before Keep");
                }
                let mut profile = registration
                    .profile
                    .context("Trial has no launch specification")?;
                profile.name = text("name")?.into();
                profile.verification = Some(SavedVerification {
                    unix_ms: unix_ms(),
                    gpu: registration.gpu,
                    rendering: verification.rendering == applications::Placement::TargetGpu,
                    decoding: verification.decoding == applications::Placement::TargetGpu,
                    other_engines: verification.other_engines == applications::Placement::TargetGpu,
                });
                let cli = value["create_launcher"]
                    .as_bool()
                    .unwrap_or(true)
                    .then(|| {
                        value["cli_path"]
                            .as_str()
                            .map(PathBuf::from)
                            .context("Missing CLI path")
                    })
                    .transpose()?;
                let outcome = launch::keep_profile(
                    &self.config,
                    &self.paths,
                    profile.clone(),
                    cli.as_deref(),
                )?;
                self.config = outcome.config;
                if let Some(launch) = self
                    .live
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Live state poisoned"))?
                    .collector
                    .launches
                    .iter_mut()
                    .find(|l| l.process == process)
                {
                    launch.profile = Some(profile);
                }
                Ok(
                    json!({"profile_saved": true, "launcher_path": outcome.launcher,"launcher_warning":outcome.warning, "verification": verification}),
                )
            }
            Operation::Profiles => Ok(serde_json::to_value(&self.config.profiles)?),
            Operation::SaveProfile => {
                let mut profile: Profile = serde_json::from_value(value)?;
                igpu_core::config::validate_profile(&profile)?;
                profile.gpu = launch::resolve_gpu(&snapshot.gpus, &profile.gpu)?
                    .pci
                    .clone();
                let mut next = self.config.clone();
                next.profiles.retain(|p| p.name != profile.name);
                next.profiles.push(profile.clone());
                next.save(&self.paths)?;
                self.config = next;
                Ok(serde_json::to_value(profile)?)
            }
            Operation::DeleteProfile => {
                let name = text("name")?;
                let mut next = self.config.clone();
                if !next.profiles.iter().any(|p| p.name == name) {
                    bail!("Profile does not exist");
                }
                next.profiles.retain(|p| p.name != name);
                next.save(&self.paths)?;
                self.config = next;
                Ok(json!(true))
            }
            Operation::RegisterLaunch => {
                let registration: LaunchRegistration = serde_json::from_value(value)?;
                let path = PathBuf::from(format!("/proc/{}/stat", registration.process.pid));
                let process = collector::read_process_stat(&path, registration.process.pid)
                    .context("Launch process disappeared")?
                    .0;
                if process != registration.process {
                    bail!("Launch process identity changed");
                }
                launch::resolve_gpu(&snapshot.gpus, &registration.gpu)?;
                {
                    let mut live = self
                        .live
                        .lock()
                        .map_err(|_| anyhow::anyhow!("Live state poisoned"))?;
                    live.collector.launches.retain(|l| l.process != process);
                    live.collector.launches.push(registration.clone());
                }
                if let Some(id) = self.active {
                    if let Some(directory) = &registration.mangohud_directory {
                        self.managed_directories
                            .insert(PathBuf::from(directory), id);
                    }
                    let launches = self
                        .live
                        .lock()
                        .map_err(|_| anyhow::anyhow!("Live state poisoned"))?
                        .collector
                        .launches
                        .clone();
                    self.store
                        .metadata(id, "launches", serde_json::to_value(launches)?)?;
                    self.store.marker(
                        id,
                        &Marker {
                            monotonic_ns: registration.anchor_ns,
                            label: format!("Launch requested on {}", registration.gpu),
                        },
                    )?;
                }
                Ok(json!(true))
            }
            Operation::Start => {
                let label = text("label")?;
                if label.len() > 256 {
                    bail!("Label too long");
                }
                Ok(json!(self.start(label, false)?))
            }
            Operation::Stop => {
                if !value["expected_id"].is_null() {
                    let expected = value["expected_id"]
                        .as_i64()
                        .context("expected_id must be an integer")?;
                    if self.active != Some(expected) {
                        bail!("Active recording changed; refusing to stop a different recording");
                    }
                }
                Ok(json!(self.stop("complete")?))
            }
            Operation::Mark => self.marker(value["label"].as_str().unwrap_or("stutter happened")),
            Operation::Recordings => Ok(serde_json::to_value(self.store.list()?)?),
            Operation::Detail => self.store.detail(id()?, self.config.stutter_threshold_ms),
            Operation::SamplesPage | Operation::Timeline => {
                bail!("Recording reads must use their independent worker")
            }
            Operation::Samples => Ok(serde_json::to_value(self.store.samples(id()?)?)?),
            Operation::Frames => Ok(serde_json::to_value(self.store.frame_logs(id()?)?)?),
            Operation::Notes => {
                self.store.notes(id()?, text("notes")?)?;
                Ok(json!(true))
            }
            Operation::Compare => {
                let ids: Vec<i64> = serde_json::from_value(value["ids"].clone())?;
                self.store.compare(&ids, self.config.stutter_threshold_ms)
            }
            Operation::Import => {
                let log = recording::parse_frame_log(
                    id()?,
                    text("filename")?.into(),
                    text("raw")?.into(),
                    value["anchor_ns"].as_u64(),
                    value["uncertainty_ms"].as_f64(),
                )?;
                self.store.import(&log)?;
                Ok(json!(true))
            }
            Operation::Export => {
                self.store
                    .export(id()?, &PathBuf::from(text("destination")?))?;
                Ok(json!(true))
            }
            Operation::DeleteRecording => {
                self.store.delete(id()?)?;
                Ok(json!(true))
            }
            Operation::Experiment => Ok(serde_json::to_value(recording::experiment_template())?),
        }
    }
}

struct Service {
    live: Arc<Mutex<live::Live>>,
    writer: worker::Writer,
    read_path: PathBuf,
    jobs: Arc<tokio::sync::Semaphore>,
    writes: Arc<tokio::sync::Semaphore>,
}

fn task_failure(error: tokio::task::JoinError, worker: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "{worker} worker failed (panic: {}, cancelled: {})",
        error.is_panic(),
        error.is_cancelled()
    )
}

#[zbus::interface(name = "io.github.igpu_man.Collector1")]
impl Service {
    #[zbus(property)]
    fn api_version(&self) -> u32 {
        igpu_core::API_VERSION
    }
    async fn request(&self, operation: &str, payload: &str) -> zbus::fdo::Result<String> {
        if payload.len()
            > if operation == "import" {
                70 * 1024 * 1024
            } else {
                1024 * 1024
            }
        {
            igpu_core::logging::event(
                "warn",
                "api.request_rejected",
                json!({"reason":"oversized"}),
            );
            return Err(zbus::fdo::Error::LimitsExceeded(
                "Request exceeds the operation envelope limit (70 MiB for import, 1 MiB otherwise)"
                    .into(),
            ));
        }
        let mut value: Value = serde_json::from_str(payload).map_err(|_| {
            igpu_core::logging::event(
                "warn",
                "api.request_rejected",
                json!({"reason":"invalid_json"}),
            );
            zbus::fdo::Error::InvalidArgs("Invalid JSON request".into())
        })?;
        if !value.is_object() {
            igpu_core::logging::event(
                "warn",
                "api.request_rejected",
                json!({"reason":"invalid_envelope"}),
            );
            return Err(zbus::fdo::Error::InvalidArgs("JSON object required".into()));
        }
        let request_id = value["_request_id"]
            .as_str()
            .filter(|s| s.len() <= 96 && s.bytes().all(|c| c.is_ascii_digit() || c == b'-'))
            .unwrap_or("untracked")
            .to_owned();
        value
            .as_object_mut()
            .expect("object envelope validated")
            .remove("_request_id");
        let parsed_operation = Operation::parse(operation);
        let observed_operation = parsed_operation
            .as_ref()
            .map(|op| op.as_str())
            .unwrap_or("unknown");
        let started = std::time::Instant::now();
        let mutation = parsed_operation.as_ref().is_ok_and(|op| op.mutation());
        let result = match parsed_operation {
            Ok(operation) => self.dispatch(operation, value).await,
            Err(error) => Err(error),
        }
        .map_err(|error| zbus::fdo::Error::Failed(igpu_core::logging::diagnostic_message(&error)));
        let millis = started.elapsed().as_secs_f64() * 1000.;
        if result.is_err() || mutation || millis > 250.0 {
            let mut fields = json!({"operation":observed_operation,"request_id":request_id,"duration_ms":millis,"outcome":if result.is_ok(){"ok"}else{"error"}});
            if let Err(error) = &result {
                fields["error"] =
                    igpu_core::logging::error_fields(&anyhow::anyhow!(error.to_string()));
            }
            igpu_core::logging::event(
                if result.is_err() || millis > 250.0 {
                    "warn"
                } else {
                    "info"
                },
                "api.request_completed",
                fields,
            );
        }
        result
    }
}

// Async I/O/control needs modest parallelism; filesystem/CSV/SQLite work runs
// on separate blocking workers or the ordered writer, not on these executors.
#[tokio::main(worker_threads = 2)]
async fn main() -> std::process::ExitCode {
    igpu_core::logging::init("daemon");
    let result = run().await;
    if let Err(error) = &result {
        igpu_core::logging::event(
            "error",
            "daemon.failed",
            igpu_core::logging::error_fields(error),
        );
    }
    if let Err(error) = result {
        eprintln!(
            "igpu-mand: {}",
            igpu_core::logging::diagnostic_message(&error)
        );
        std::process::ExitCode::FAILURE
    } else {
        std::process::ExitCode::SUCCESS
    }
}

#[derive(clap::Parser)]
#[command(name="igpu-mand",version,long_version=igpu_core::build::LONG_VERSION,about="Unprivileged GPU telemetry collector")]
struct Options {}
async fn run() -> Result<()> {
    use clap::Parser;
    let _options = Options::try_parse().unwrap_or_else(|error| {
        if error.use_stderr() {
            igpu_core::logging::event(
                "warn",
                "daemon.arguments_rejected",
                json!({"kind":format!("{:?}",error.kind())}),
            );
        }
        error.exit()
    });
    let paths = Paths::discover()?;
    let config = Config::load(&paths)?;
    let store = Store::open(
        &paths.data.join("recordings.sqlite"),
        config.storage_limit_mb,
    )?;
    let (config, recovery_warnings) = launch::recover_pending_launchers(&config, &paths);
    let mut diagnostics_cache = collector::DiagnosticsCache::default();
    let mut collector = Collector::new("/sys", "/proc", diagnostics_cache.collect());
    let snapshot = Arc::new(collector.sample(&config));
    let live = Arc::new(Mutex::new(live::Live {
        collector,
        config: config.clone(),
        snapshot,
        ring: VecDeque::new(),
        active: None,
        stopping: None,
        wake: Arc::new(tokio::sync::Notify::new()),
        health: recovery_warnings.clone(),
        update_lease: boot_update_lease()?,
    }));
    let read_path = paths.data.join("recordings.sqlite");
    let state = State {
        live: live.clone(),
        config,
        paths,
        store,
        active: None,
        deadline: None,
        last_saved_ns: 0,
        writer_delayed: false,
        health: recovery_warnings,
        imported: BTreeSet::new(),
        frame_anchors: Default::default(),
        managed_directories: Default::default(),
    };
    let (writer, mut completion) = worker::Writer::spawn(state);
    let service = Service {
        live: live.clone(),
        writer: writer.clone(),
        read_path,
        jobs: Arc::new(tokio::sync::Semaphore::new(2)),
        writes: Arc::new(tokio::sync::Semaphore::new(8)),
    };
    let _connection = zbus::connection::Builder::session()?
        .name(igpu_core::BUS)?
        .serve_at(igpu_core::OBJECT, service)?
        .build()
        .await?;
    let sampler_live = live.clone();
    let sampler_writer = writer.clone();
    let wake = live
        .lock()
        .map_err(|_| anyhow::anyhow!("Live state poisoned"))?
        .wake
        .clone();
    let mut sampler = tokio::spawn(async move {
        let mut notified = BTreeSet::new();
        loop {
            let started = std::time::Instant::now();
            let live = sampler_live.clone();
            let writer = sampler_writer.clone();
            let interval = tokio::task::spawn_blocking(move || -> Result<(u64, Vec<String>)> {
                let mut live = live
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Live state poisoned"))?;
                let config = live.config.clone();
                let mut sample = live.collector.sample(&config);
                sample.active_recording = live.active.or(live.stopping);
                sample.diagnostics.notes.extend(live.health.iter().cloned());
                if let Some(id) = live.stopping {
                    sample.diagnostics.notes.push(format!(
                        "Recording {id} is stopping; buffered writes are draining."
                    ));
                }
                let notices = if config.notifications {
                    sample.recommendations.clone()
                } else {
                    vec![]
                };
                let interval = if live.active.is_some() {
                    config.recording_interval_ms
                } else {
                    config.sample_interval_ms
                };
                let sample = Arc::new(sample);
                live.ring.push_back(sample.clone());
                while live.ring.front().is_some_and(|s| {
                    sample.monotonic_ns.saturating_sub(s.monotonic_ns) > 120_000_000_000
                }) || live.ring.len() > 1201
                {
                    live.ring.pop_front();
                }
                live.snapshot = sample.clone();
                if live.active.is_some() {
                    writer.sample(sample)?;
                }
                Ok((interval, notices))
            })
            .await
            .map_err(|e| task_failure(e, "Sampler"))??;
            for notice in interval.1 {
                if notified.insert(notice.clone()) {
                    std::thread::spawn(move || {
                        collector::command_output("notify-send", &["igpu-man", &notice]);
                    });
                }
            }
            tokio::select! {_=tokio::time::sleep(Duration::from_millis(interval.0).saturating_sub(started.elapsed()))=>{},_=wake.notified()=>{}}
        }
        #[allow(unreachable_code)]
        Ok::<(), anyhow::Error>(())
    });
    let diagnostic_live = live.clone();
    let mut diagnostics = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            let live = diagnostic_live.clone();
            diagnostics_cache = tokio::task::spawn_blocking(move || -> Result<_> {
                let diagnostics = diagnostics_cache.collect();
                live.lock()
                    .map_err(|_| anyhow::anyhow!("Live state poisoned"))?
                    .collector
                    .diagnostics = diagnostics;
                Ok(diagnostics_cache)
            })
            .await
            .map_err(|e| task_failure(e, "Diagnostics"))??;
        }
        #[allow(unreachable_code)]
        Ok::<(), anyhow::Error>(())
    });
    igpu_core::logging::event(
        "info",
        "daemon.ready",
        json!({"api_version":igpu_core::API_VERSION}),
    );
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let failure = tokio::select! {
        _=tokio::signal::ctrl_c()=>None,
        _=terminate.recv()=>None,
        result=&mut sampler=>Some(result.map_err(|e|task_failure(e,"Sampler supervisor")).and_then(|r|r).err().unwrap_or_else(||anyhow::anyhow!("Sampler exited unexpectedly"))),
        result=&mut diagnostics=>Some(result.map_err(|e|task_failure(e,"Diagnostics supervisor")).and_then(|r|r).err().unwrap_or_else(||anyhow::anyhow!("Diagnostics exited unexpectedly"))),
        result=&mut completion=>Some(result.context("Recording worker panicked or disconnected").and_then(|r|r).err().unwrap_or_else(||anyhow::anyhow!("Recording worker exited unexpectedly"))),
    };
    sampler.abort();
    diagnostics.abort();
    {
        let mut live = live
            .lock()
            .map_err(|_| anyhow::anyhow!("Live state poisoned during shutdown"))?;
        live.active = None;
    }
    tokio::time::timeout(Duration::from_secs(30), writer.shutdown())
        .await
        .context("Recording shutdown exceeded 30 seconds")??;
    igpu_core::logging::event(
        "info",
        "daemon.stopped",
        json!({"worker_failure":failure.is_some()}),
    );
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(())
}
