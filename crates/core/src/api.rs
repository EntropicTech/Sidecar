//! Typed operation names for the versioned JSON/D-Bus boundary.
use anyhow::{bail, Result};
macro_rules! operations {
    ($($variant:ident => $wire:literal,)*)=>{
        #[derive(Debug,Clone,Copy,PartialEq,Eq,serde::Serialize,serde::Deserialize)]
        pub enum Operation {$(#[serde(rename=$wire)] $variant,)*}
        impl Operation {
            pub const ALL:&'static[Self]=&[$(Self::$variant,)*];
            pub fn as_str(self)->&'static str {match self {$(Self::$variant=>$wire,)*}}
            pub fn parse(name:&str)->Result<Self> {match name {$($wire=>Ok(Self::$variant),)*_=>bail!("Unknown API operation")}}
        }
    }
}
operations! {
    Capabilities => "capabilities",
    PrepareUpdate => "prepare_update",
    CancelUpdate => "cancel_update",
    PendingTrials => "pending_trials",
    DiscardTrial => "discard_trial",
    Snapshot => "snapshot",
    Candidates => "candidates",
    Launchers => "launchers",
    Management => "management",
    SetManagement => "set_management",
    VerifyTrial => "verify_trial",
    KeepTrial => "keep_trial",
    AppRoutePreview => "app_route_preview",
    AppRouteApply => "app_route_apply",
    NormalLaunchRemoveLogin => "normal_launch_remove_login",
    NormalLaunchPreview => "normal_launch_preview",
    NormalLaunchStatus => "normal_launch_status",
    NormalLaunchApply => "normal_launch_apply",
    NormalLaunchResume => "normal_launch_resume",
    NormalLaunchUndo => "normal_launch_undo",
    Profiles => "profiles",
    SaveProfile => "save_profile",
    DeleteProfile => "delete_profile",
    CreateLauncher => "create_launcher",
    RegisterLaunch => "register_launch",
    Start => "start",
    Stop => "stop",
    Mark => "mark",
    Recordings => "recordings",
    Detail => "detail",
    Samples => "samples",
    SamplesPage => "samples_page",
    Timeline => "timeline",
    Frames => "frames",
    Notes => "notes",
    Compare => "compare",
    Import => "import",
    Export => "export",
    DeleteRecording => "delete_recording",
    Experiment => "experiment",
}
impl std::fmt::Display for Operation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl Operation {
    pub fn live(self) -> bool {
        matches!(
            self,
            Self::Snapshot
                | Self::Candidates
                | Self::Profiles
                | Self::Management
                | Self::PendingTrials
                | Self::VerifyTrial
        )
    }
    pub fn read_job(self) -> bool {
        matches!(
            self,
            Self::AppRoutePreview
                | Self::NormalLaunchPreview
                | Self::NormalLaunchStatus
                | Self::Recordings
                | Self::Detail
                | Self::Samples
                | Self::SamplesPage
                | Self::Timeline
                | Self::Frames
                | Self::Compare
                | Self::Export
                | Self::Launchers
                | Self::Experiment
        )
    }
    pub fn recording(self) -> bool {
        matches!(
            self,
            Self::Start
                | Self::Stop
                | Self::Mark
                | Self::Notes
                | Self::Import
                | Self::Export
                | Self::DeleteRecording
                | Self::Detail
                | Self::Compare
                | Self::Recordings
        )
    }
    pub fn large(self) -> bool {
        matches!(
            self,
            Self::AppRoutePreview
                | Self::AppRouteApply
                | Self::NormalLaunchRemoveLogin
                | Self::NormalLaunchStatus
                | Self::NormalLaunchPreview
                | Self::NormalLaunchApply
                | Self::NormalLaunchResume
                | Self::NormalLaunchUndo
                | Self::Import
                | Self::Export
                | Self::Samples
                | Self::SamplesPage
                | Self::Timeline
                | Self::Frames
                | Self::Detail
                | Self::Compare
                | Self::Start
                | Self::Stop
                | Self::Mark
        )
    }
    pub fn mutation(self) -> bool {
        matches!(
            self,
            Self::AppRouteApply
                | Self::NormalLaunchRemoveLogin
                | Self::NormalLaunchApply
                | Self::NormalLaunchResume
                | Self::NormalLaunchUndo
                | Self::PrepareUpdate
                | Self::CancelUpdate
                | Self::DiscardTrial
                | Self::SetManagement
                | Self::KeepTrial
                | Self::SaveProfile
                | Self::DeleteProfile
                | Self::CreateLauncher
                | Self::RegisterLaunch
                | Self::Start
                | Self::Stop
                | Self::Mark
                | Self::Notes
                | Self::Import
                | Self::Export
                | Self::DeleteRecording
        )
    }
}

/// Publish a shortcut for the authoritative saved profile, never a caller copy.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateLauncher {
    pub name: String,
    pub cli_path: std::path::PathBuf,
}
