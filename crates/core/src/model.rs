use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Available,
    Idle,
    Unavailable,
    PermissionDenied,
    Stale,
    WarmingUp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metric {
    pub value: Option<f64>,
    pub unit: String,
    pub source: String,
    pub status: Availability,
}

impl Metric {
    pub fn measured(value: f64, unit: &str, source: impl Into<String>) -> Self {
        Self {
            value: Some(value),
            unit: unit.into(),
            source: source.into(),
            status: if value == 0.0 {
                Availability::Idle
            } else {
                Availability::Available
            },
        }
    }
    pub fn missing(unit: &str, source: impl Into<String>, status: Availability) -> Self {
        Self {
            value: None,
            unit: unit.into(),
            source: source.into(),
            status,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Output {
    pub connector: String,
    pub connected: bool,
    pub advertised_modes: Vec<String>,
    pub current_mode: Option<String>,
    pub refresh_hz: Option<f64>,
    pub scale: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gpu {
    pub pci: String,
    pub name: String,
    pub alias: String,
    pub integrated: bool,
    pub card: String,
    pub render_node: Option<String>,
    pub capabilities: Vec<String>,
    pub outputs: Vec<Output>,
    pub metrics: BTreeMap<String, Metric>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProcessId {
    pub pid: u32,
    pub start_ticks: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Process {
    pub id: ProcessId,
    pub parent: u32,
    pub executable: String,
    pub name: String,
    pub cpu_percent: Metric,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GpuUsage {
    pub opened: bool,
    pub engines: BTreeMap<String, Metric>,
    pub memory: BTreeMap<String, Metric>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Application {
    pub id: ProcessId,
    /// Identity of the registered launcher, including when only descendants remain.
    #[serde(default)]
    pub launch_id: Option<ProcessId>,
    #[serde(default)]
    pub profile_name: Option<String>,
    pub name: String,
    pub executable: String,
    pub processes: Vec<Process>,
    pub requested_gpu: Option<String>,
    pub observed_gpus: Vec<String>,
    pub placement: String,
    pub gpus: BTreeMap<String, GpuUsage>,
    pub cpu_percent: Metric,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Coverage {
    pub fdinfo_scan_interval_ms: u64,
    pub processes_seen: usize,
    pub processes_inspected: usize,
    pub permission_denied: usize,
    pub vanished: usize,
    pub clients_observed: usize,
    pub unidentifiable_clients: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostics {
    pub session: String,
    pub kwin_support: Option<String>,
    pub display_configuration: Option<serde_json::Value>,
    pub versions: BTreeMap<String, String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub api_version: u32,
    pub monotonic_ns: u64,
    pub unix_ms: u64,
    pub gpus: Vec<Gpu>,
    pub applications: Vec<Application>,
    pub coverage: Coverage,
    pub diagnostics: Diagnostics,
    pub recommendations: Vec<String>,
    pub active_recording: Option<i64>,
    pub collector_duration_ms: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    pub executable: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Stored PCI address; aliases are resolved before saving.
    pub gpu: String,
    #[serde(default)]
    pub vulkan_only: bool,
    #[serde(default)]
    pub firefox: bool,
    #[serde(default)]
    pub record: bool,
    #[serde(default)]
    pub mangohud: bool,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    #[serde(default)]
    pub working_directory: Option<String>,
    #[serde(default)]
    pub desktop: Option<DesktopMetadata>,
    #[serde(default)]
    pub verification: Option<SavedVerification>,
    /// A durable Keep intent, cleared after the derived launcher is published.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_launcher: Option<PendingLauncher>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingLauncher {
    pub schema_version: u32,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopMetadata {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub mime_types: String,
    pub categories: String,
    /// File/URL arguments are inserted here; static arguments stay intact.
    pub file_argument: Option<(usize, char)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedVerification {
    pub unix_ms: u64,
    pub gpu: String,
    pub rendering: bool,
    pub decoding: bool,
    pub other_engines: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchRegistration {
    pub process: ProcessId,
    pub gpu: String,
    pub profile: Option<Profile>,
    pub mangohud_directory: Option<String>,
    pub anchor_ns: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Marker {
    pub monotonic_ns: u64,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recording {
    pub id: i64,
    pub label: String,
    pub started_ns: u64,
    pub finished_ns: Option<u64>,
    pub state: String,
    pub notes: String,
    pub metadata: serde_json::Value,
    pub samples: usize,
    pub markers: Vec<Marker>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameLog {
    pub recording_id: i64,
    pub filename: String,
    pub raw: String,
    /// Monotonic timestamp corresponding to elapsed=0; absent means unaligned.
    pub anchor_ns: Option<u64>,
    pub uncertainty_ms: Option<f64>,
    pub frames: Vec<Frame>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    pub elapsed_ns: u64,
    pub frametime_ms: f64,
}

pub fn monotonic_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // CLOCK_MONOTONIC shares a time domain across the collector and launch wrapper.
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
    }
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

pub fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// GPU-only view data; complete process telemetry stays in the recording/export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineSample {
    pub monotonic_ns: u64,
    pub gpus: Vec<Gpu>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameSeries {
    #[serde(default)]
    pub total_frames: usize,
    #[serde(default)]
    pub aggregation: String,
    pub filename: String,
    pub anchor_ns: Option<u64>,
    pub uncertainty_ms: Option<f64>,
    pub frames: Vec<Frame>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordingTimeline {
    pub samples: Vec<TimelineSample>,
    pub frames: Vec<FrameSeries>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleCursor {
    pub monotonic_ns: u64,
    pub row_id: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplePage {
    pub samples: Vec<Snapshot>,
    pub next_cursor: Option<SampleCursor>,
    pub through_ns: u64,
}
impl From<Snapshot> for TimelineSample {
    fn from(s: Snapshot) -> Self {
        Self {
            monotonic_ns: s.monotonic_ns,
            gpus: s.gpus,
        }
    }
}
impl From<FrameLog> for FrameSeries {
    fn from(log: FrameLog) -> Self {
        let total_frames = log.frames.len();
        Self {
            total_frames,
            aggregation: "max_per_contiguous_bucket".into(),
            filename: log.filename,
            anchor_ns: log.anchor_ns,
            uncertainty_ms: log.uncertainty_ms,
            frames: crate::recording::frame_peaks(&log.frames, 3000),
        }
    }
}
