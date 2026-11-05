//! The sampler's only shared lock. Never perform recording or launcher file I/O here.
use super::*;

pub struct Live {
    pub collector: Collector,
    pub config: Config,
    pub snapshot: Arc<Snapshot>,
    pub ring: VecDeque<Arc<Snapshot>>,
    pub active: Option<i64>,
    pub stopping: Option<i64>,
    pub wake: Arc<tokio::sync::Notify>,
    pub health: Vec<String>,
    pub update_lease: Option<(String, u64)>,
}
impl Live {
    pub fn check_update(&mut self, operation: Operation, token: Option<&str>) -> Result<()> {
        if self
            .update_lease
            .as_ref()
            .is_some_and(|(_, until)| monotonic_ns() >= *until)
        {
            self.update_lease = None;
        }
        let restoring = operation == Operation::RegisterLaunch
            && self
                .update_lease
                .as_ref()
                .is_some_and(|(held, _)| Some(held.as_str()) == token);
        if self.update_lease.is_some()
            && !restoring
            && !matches!(
                operation,
                Operation::PrepareUpdate | Operation::CancelUpdate
            )
        {
            bail!("An update is in progress; no mutation was applied. Retry after the update completes");
        }
        Ok(())
    }
    pub fn view(&self) -> View {
        View {
            snapshot: self.snapshot.clone(),
            ring: self.ring.clone(),
            config: self.config.clone(),
            launches: self.collector.launches.clone(),
        }
    }
}
pub struct View {
    pub snapshot: Arc<Snapshot>,
    pub ring: VecDeque<Arc<Snapshot>>,
    pub config: Config,
    pub launches: Vec<LaunchRegistration>,
}
impl View {
    pub fn request(self, operation: Operation) -> Result<Value> {
        match operation {
            Operation::Snapshot => {
                let mut snapshot = (*self.snapshot).clone();
                snapshot.active_recording = self.snapshot.active_recording;
                Ok(serde_json::to_value(snapshot)?)
            }
            Operation::Candidates => {
                let mut candidates =
                    applications::candidates(&self.snapshot, &self.ring, &self.config);
                for candidate in &mut candidates {
                    if let Some(registration) = candidate
                        .launch_path
                        .registration
                        .as_ref()
                        .and_then(|id| self.launches.iter().find(|l| &l.process == id))
                    {
                        let saved = registration.profile.as_ref().is_some_and(|p| {
                            self.config
                                .profiles
                                .iter()
                                .any(|saved| saved.name == p.name && same_launch_settings(saved, p))
                        });
                        let igpu = self
                            .snapshot
                            .gpus
                            .iter()
                            .any(|g| g.integrated && g.pci == registration.gpu);
                        if saved && igpu {
                            candidate.verification = Some(self.verify(&registration.process));
                        }
                    }
                }
                Ok(serde_json::to_value(candidates)?)
            }
            Operation::Profiles => Ok(serde_json::to_value(&self.config.profiles)?),
            Operation::Management => Ok(serde_json::to_value(&self.config.managed_elsewhere)?),
            Operation::PendingTrials => Ok(serde_json::to_value(
                self.launches
                    .iter()
                    .filter(|l| {
                        l.profile.as_ref().is_some_and(|p| {
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
            _ => bail!("Unknown live operation"),
        }
    }
    pub fn verify(&self, process: &ProcessId) -> applications::Verification {
        if let Some(registration) = self.launches.iter().find(|l| &l.process == process) {
            applications::verify_trial(
                &self.snapshot,
                &self.ring,
                process,
                &registration.gpu,
                registration.anchor_ns,
            )
        } else {
            applications::Verification {application_seen:false,rendering:applications::Placement::Waiting,decoding:applications::Placement::Waiting,other_engines:applications::Placement::Waiting,can_keep:false,evidence_seconds:0.,explanation:"Launch is not tracked; repeat the trial if the collector restarted or the app exited.".into()}
        }
    }
}

// A reused settings name must not inherit evidence from another launch intent.
fn same_launch_settings(a: &Profile, b: &Profile) -> bool {
    a.executable == b.executable
        && a.args == b.args
        && a.gpu == b.gpu
        && a.environment == b.environment
        && a.working_directory == b.working_directory
        && a.vulkan_only == b.vulkan_only
        && a.firefox == b.firefox
        && a.record == b.record
        && a.mangohud == b.mangohud
        && a.desktop.as_ref().and_then(|d| d.file_argument)
            == b.desktop.as_ref().and_then(|d| d.file_argument)
}
