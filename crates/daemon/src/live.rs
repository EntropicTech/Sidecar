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
            Operation::Candidates => Ok(serde_json::to_value(applications::candidates(
                &self.snapshot,
                &self.ring,
                &self.config,
            ))?),
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
