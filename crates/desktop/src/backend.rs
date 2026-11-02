//! Dashboard view-model and collection health, independent of widgets.
use super::*;
#[derive(Default)]
pub(super) struct Backend {
    pub(super) snapshot: Option<Arc<Snapshot>>,
    pub(super) profiles: Vec<Profile>,
    pub(super) recordings: Arc<Vec<Recording>>,
    pub(super) result: String,
    pub(super) error: String,
    pub(super) connection_error: String,
    pub(super) health_errors: BTreeMap<&'static str, String>,
    pub(super) last_success: Option<Instant>,
    pub(super) detail: Option<Arc<Value>>,
    pub(super) comparison: Option<Arc<Value>>,
    pub(super) samples: Arc<Vec<TimelineSample>>,
    pub(super) frames: Arc<Vec<FrameSeries>>,
    pub(super) candidates: Vec<igpu_core::applications::Candidate>,
    pub(super) launchers: Vec<igpu_core::launchers::InstalledLauncher>,
    pub(super) trial: Option<Trial>,
    pub(super) pending_trials: Vec<LaunchRegistration>,
    pub(super) trial_pending: bool,
    pub(super) trial_keep_pending: bool,
    pub(super) managed_elsewhere: Vec<String>,
    pub(super) workflow_notice: String,
    pub(super) recording_generation: u64,
    pub(super) recording_busy: bool,
}

impl Backend {
    pub(super) fn apply_timeline(
        &mut self,
        samples: anyhow::Result<Vec<TimelineSample>>,
        frames: anyhow::Result<Vec<FrameSeries>>,
    ) {
        match (samples, frames) {
            (Ok(samples), Ok(frames)) => {
                self.samples = Arc::new(samples);
                self.frames = Arc::new(frames);
            }
            (Err(error), _) | (_, Err(error)) => {
                self.samples = Arc::default();
                self.frames = Arc::default();
                self.error = format!("Timeline unavailable: {error:#}");
                igpu_core::logging::event(
                    "warn",
                    "dashboard.timeline_unavailable",
                    igpu_core::logging::error_fields(&error),
                );
            }
        }
    }
}
pub(super) fn backend_failure(state: &mut Backend, operation: &'static str, error: &anyhow::Error) {
    let first = state
        .health_errors
        .insert(operation, format!("{operation} unavailable: {error:#}"))
        .is_none();
    if first {
        igpu_core::logging::event(
            "warn",
            "dashboard.backend_unavailable",
            json!({"operation":operation,"error":igpu_core::logging::error_fields(error)}),
        );
    }
}
pub(super) fn backend_recovered(state: &mut Backend, operation: &'static str) {
    if state.health_errors.remove(operation).is_some() {
        igpu_core::logging::event(
            "info",
            "dashboard.backend_recovered",
            json!({"operation":operation}),
        );
    }
}
