use crate::{
    model::*,
    recording::{self, FrameSummary},
};
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OpenFlags};
use serde::Serialize;
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, path::Path};

pub struct Store {
    db: Connection,
    limit_bytes: u64,
    _lock: Option<fs::File>,
    read_deadline: Option<std::time::Instant>,
}

impl Store {
    pub fn open(path: &Path, limit_mb: u64) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        }
        use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path.with_extension("lock"))?;
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            bail!("Recording database is already in use by another collector");
        }
        let db = Connection::open(path)?;
        let version: u32 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > 1 {
            bail!("Unsupported recording database schema {version}");
        }
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.busy_timeout(std::time::Duration::from_millis(250))?;
        db.pragma_update(None, "foreign_keys", "ON")?;
        db.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS recordings (
            id INTEGER PRIMARY KEY, label TEXT NOT NULL, started_ns INTEGER NOT NULL,
            finished_ns INTEGER, state TEXT NOT NULL, notes TEXT NOT NULL DEFAULT '', metadata TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS samples (recording_id INTEGER NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
                monotonic_ns INTEGER NOT NULL, json TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS samples_recording ON samples(recording_id, monotonic_ns);
            CREATE TABLE IF NOT EXISTS markers (recording_id INTEGER NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
                monotonic_ns INTEGER NOT NULL, label TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS frame_logs (id INTEGER PRIMARY KEY,
                recording_id INTEGER NOT NULL REFERENCES recordings(id) ON DELETE CASCADE, json TEXT NOT NULL);")?;
        db.pragma_update(None, "user_version", 1)?;
        // A previous process may have crashed. Never silently resume its recording.
        db.execute(
            "UPDATE recordings SET state='interrupted' WHERE state='recording'",
            [],
        )?;
        Ok(Self {
            db,
            limit_bytes: limit_mb * 1024 * 1024,
            _lock: Some(lock),
            read_deadline: None,
        })
    }
    /// A separate query-only WAL connection. No ownership lock, schema changes,
    /// interrupted-recording recovery, or writes on this path.
    pub fn reader(path: &Path) -> Result<Self> {
        let db = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        db.pragma_update(None, "query_only", "ON")?;
        db.pragma_update(None, "cache_size", -1024)?;
        db.busy_timeout(std::time::Duration::from_millis(250))?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        db.progress_handler(1000, Some(move || std::time::Instant::now() >= deadline));
        db.execute_batch("BEGIN DEFERRED")?;
        Ok(Self {
            db,
            limit_bytes: 0,
            _lock: None,
            read_deadline: Some(deadline),
        })
    }
    fn check_read(&self) -> Result<()> {
        anyhow::ensure!(
            self.read_deadline
                .is_none_or(|d| std::time::Instant::now() < d),
            "Recording read exceeded its 20-second work budget"
        );
        Ok(())
    }
    pub fn serialized_import(&self, id: i64, text: &str) -> Result<()> {
        self.get(id)?;
        self.room(text.len())?;
        self.db.execute(
            "INSERT INTO frame_logs(recording_id,json) VALUES(?1,?2)",
            params![id, text],
        )?;
        Ok(())
    }
    fn room(&self, bytes: usize) -> Result<()> {
        let pages: u64 = self
            .db
            .pragma_query_value(None, "page_count", |r| r.get(0))?;
        let free: u64 = self
            .db
            .pragma_query_value(None, "freelist_count", |r| r.get(0))?;
        let size: u64 = self
            .db
            .pragma_query_value(None, "page_size", |r| r.get(0))?;
        if pages.saturating_sub(free) * size + bytes as u64 + size * 4 > self.limit_bytes {
            bail!(
                "Recording storage limit reached; delete recordings or increase storage_limit_mb"
            );
        }
        Ok(())
    }
    pub fn start(&self, label: &str, started: u64, metadata: &Value) -> Result<i64> {
        let data = serde_json::to_string(metadata)?;
        self.room(data.len())?;
        self.db.execute(
            "INSERT INTO recordings(label,started_ns,state,metadata) VALUES(?1,?2,'recording',?3)",
            params![label, started, data],
        )?;
        Ok(self.db.last_insert_rowid())
    }
    pub fn sample(&self, id: i64, sample: &Snapshot) -> Result<()> {
        let text = serde_json::to_string(sample)?;
        self.room(text.len())?;
        self.db.execute(
            "INSERT INTO samples VALUES(?1,?2,?3)",
            params![id, sample.monotonic_ns, text],
        )?;
        Ok(())
    }
    pub fn stop(&self, id: i64, now: u64, state: &str) -> Result<()> {
        self.db.execute(
            "UPDATE recordings SET finished_ns=?1,state=?2 WHERE id=?3",
            params![now, state, id],
        )?;
        Ok(())
    }
    pub fn marker(&self, id: i64, marker: &Marker) -> Result<()> {
        self.room(marker.label.len())?;
        self.db.execute(
            "INSERT INTO markers VALUES(?1,?2,?3)",
            params![id, marker.monotonic_ns, marker.label],
        )?;
        Ok(())
    }
    pub fn notes(&self, id: i64, notes: &str) -> Result<()> {
        self.get(id)?;
        self.room(notes.len())?;
        self.db.execute(
            "UPDATE recordings SET notes=?1 WHERE id=?2",
            params![notes, id],
        )?;
        Ok(())
    }
    pub fn metadata(&self, id: i64, key: &str, value: Value) -> Result<()> {
        let mut recording = self.get(id)?;
        recording.metadata[key] = value;
        let text = serde_json::to_string(&recording.metadata)?;
        self.room(text.len())?;
        self.db.execute(
            "UPDATE recordings SET metadata=?1 WHERE id=?2",
            params![text, id],
        )?;
        Ok(())
    }
    pub fn list(&self) -> Result<Vec<Recording>> {
        self.check_read()?;
        let bytes: u64 = self.db.query_row(
            "SELECT coalesce(sum(length(metadata)+length(notes)+length(label)),0) FROM recordings",
            [],
            |r| r.get(0),
        )?;
        anyhow::ensure!(
            bytes <= 64 * 1024 * 1024,
            "Recording catalog exceeds 64 MiB; inspect recordings individually"
        );
        let mut statement = self
            .db
            .prepare("SELECT id FROM recordings ORDER BY id DESC")?;
        let ids: Vec<i64> = statement
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        ids.into_iter().map(|id| self.get(id)).collect()
    }
    pub fn get(&self, id: i64) -> Result<Recording> {
        self.check_read()?;
        let marker_bytes: u64 = self.db.query_row(
            "SELECT coalesce(sum(length(label)+32),0) FROM markers WHERE recording_id=?1",
            [id],
            |r| r.get(0),
        )?;
        anyhow::ensure!(
            marker_bytes <= 16 * 1024 * 1024,
            "Recording marker data exceeds 16 MiB"
        );
        let row = self.db.query_row("SELECT label,started_ns,finished_ns,state,notes,metadata FROM recordings WHERE id=?1", [id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?, r.get::<_, Option<u64>>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, String>(5)?))
        }).context("Recording does not exist")?;
        let mut statement = self.db.prepare(
            "SELECT monotonic_ns,label FROM markers WHERE recording_id=?1 ORDER BY monotonic_ns",
        )?;
        let markers = statement
            .query_map([id], |r| {
                Ok(Marker {
                    monotonic_ns: r.get(0)?,
                    label: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let samples = self.db.query_row(
            "SELECT count(*) FROM samples WHERE recording_id=?1",
            [id],
            |r| r.get::<_, usize>(0),
        )?;
        Ok(Recording {
            id,
            label: row.0,
            started_ns: row.1,
            finished_ns: row.2,
            state: row.3,
            notes: row.4,
            metadata: serde_json::from_str(&row.5)?,
            samples,
            markers,
        })
    }
    pub fn samples(&self, id: i64) -> Result<Vec<Snapshot>> {
        self.check_read()?;
        let mut stmt = self
            .db
            .prepare("SELECT json FROM samples WHERE recording_id=?1 ORDER BY monotonic_ns")?;
        self.get(id)?;
        let mut rows = stmt.query([id])?;
        let mut bytes = 0usize;
        let mut values = Vec::new();
        while let Some(row) = rows.next()? {
            self.check_read()?;
            let text: String = row.get(0)?;
            bytes += text.len();
            anyhow::ensure!(
                bytes <= 64 * 1024 * 1024,
                "Recording timeline exceeds the 64 MiB read limit; export it instead"
            );
            values.push(serde_json::from_str(&text)?);
        }
        Ok(values)
    }

    pub fn samples_page(
        &self,
        id: i64,
        after: Option<SampleCursor>,
        through: Option<u64>,
        limit: usize,
    ) -> Result<SamplePage> {
        self.check_read()?;
        anyhow::ensure!(
            (1..=128).contains(&limit),
            "Sample page limit must be 1..128"
        );
        anyhow::ensure!(
            self.get(id)?.state != "recording",
            "Stop the recording before paging raw samples"
        );
        let end: u64 = self.db.query_row(
            "SELECT coalesce(max(monotonic_ns),0) FROM samples WHERE recording_id=?1",
            [id],
            |r| r.get(0),
        )?;
        let through = through.unwrap_or(end).min(end);
        let mut stmt=self.db.prepare("SELECT monotonic_ns,json,rowid FROM samples WHERE recording_id=?1 AND (?2 IS NULL OR monotonic_ns>?2 OR (monotonic_ns=?2 AND rowid>?3)) AND monotonic_ns<=?4 ORDER BY monotonic_ns,rowid LIMIT ?5")?;
        let mut rows = stmt.query(params![
            id,
            after.as_ref().map(|c| c.monotonic_ns),
            after.as_ref().map(|c| c.row_id),
            through,
            limit + 1
        ])?;
        let mut samples = vec![];
        let mut bytes = 0;
        let mut cursor = None;
        let mut more = false;
        while let Some(row) = rows.next()? {
            self.check_read()?;
            let at: u64 = row.get(0)?;
            let raw = row.get_ref(1)?.as_str()?;
            anyhow::ensure!(
                raw.len() <= 4 * 1024 * 1024,
                "A raw sample exceeds the 4 MiB page budget; export it instead"
            );
            if samples.len() == limit
                || (!samples.is_empty() && bytes + raw.len() > 4 * 1024 * 1024)
            {
                more = true;
                break;
            }
            samples.push(serde_json::from_str(raw)?);
            bytes += raw.len();
            cursor = Some(SampleCursor {
                monotonic_ns: at,
                row_id: row.get(2)?,
            });
        }
        Ok(SamplePage {
            samples,
            next_cursor: if more { cursor } else { None },
            through_ns: through,
        })
    }
    pub fn timeline(&self, id: i64) -> Result<RecordingTimeline> {
        self.check_read()?;
        self.get(id)?;
        let mut stmt = self
            .db
            .prepare("SELECT json FROM samples WHERE recording_id=?1 ORDER BY monotonic_ns")?;
        let mut rows = stmt.query([id])?;
        let mut samples = vec![];
        while let Some(row) = rows.next()? {
            self.check_read()?;
            anyhow::ensure!(
                samples.len() < 10_000,
                "Timeline exceeds 10,000 points; use raw sample pages or export"
            );
            // Serde ignores non-GPU fields without allocating their process trees.
            let raw = row.get_ref(0)?.as_str()?;
            samples.push(serde_json::from_str::<TimelineSample>(raw)?);
        }
        let logs: usize = self.db.query_row(
            "SELECT count(*) FROM frame_logs WHERE recording_id=?1",
            [id],
            |r| r.get(0),
        )?;
        anyhow::ensure!(
            logs <= 128,
            "Recording exceeds 128 frame logs; export instead"
        );
        let plot_points = (3000 / logs.max(1)).max(1);
        let mut stmt = self
            .db
            .prepare("SELECT json FROM frame_logs WHERE recording_id=?1 ORDER BY id")?;
        let mut rows = stmt.query([id])?;
        let mut frames = vec![];
        let mut count = 0;
        while let Some(row) = rows.next()? {
            self.check_read()?;
            let raw = row.get_ref(0)?.as_str()?;
            anyhow::ensure!(
                raw.len() <= 128 * 1024 * 1024,
                "Stored frame row exceeds 128 MiB"
            );
            // The raw CSV is skipped on this UI path; original logs remain in exports.
            let mut log: FrameSeries = serde_json::from_str(raw)?;
            count += log.frames.len();
            log.total_frames = log.frames.len();
            log.aggregation = "max_per_contiguous_bucket".into();
            log.frames = recording::frame_peaks(&log.frames, plot_points);
            anyhow::ensure!(
                count <= 1_000_000,
                "Timeline exceeds one million frames; export it instead"
            );
            frames.push(log);
        }
        Ok(RecordingTimeline { samples, frames })
    }
    pub fn import(&self, log: &FrameLog) -> Result<()> {
        self.get(log.recording_id)?;
        let text = bounded_json(log, 128 * 1024 * 1024)?;
        self.room(text.len())?;
        self.db.execute(
            "INSERT INTO frame_logs(recording_id,json) VALUES(?1,?2)",
            params![log.recording_id, text],
        )?;
        Ok(())
    }
    pub fn frame_logs(&self, id: i64) -> Result<Vec<FrameLog>> {
        self.check_read()?;
        let mut statement = self
            .db
            .prepare("SELECT json FROM frame_logs WHERE recording_id=?1 ORDER BY id")?;
        let mut rows = statement.query([id])?;
        let mut bytes = 0usize;
        let mut values = Vec::new();
        while let Some(row) = rows.next()? {
            self.check_read()?;
            let text: String = row.get(0)?;
            bytes += text.len();
            anyhow::ensure!(
                bytes <= 64 * 1024 * 1024,
                "Frame data exceeds the 64 MiB read limit; export it instead"
            );
            values.push(serde_json::from_str(&text)?);
        }
        Ok(values)
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        if self.get(id)?.state == "recording" {
            bail!("Stop recording before deleting it");
        }
        self.db
            .execute("DELETE FROM recordings WHERE id=?1", [id])?;
        self.db.execute_batch("PRAGMA incremental_vacuum;")?;
        Ok(())
    }
    fn frame_summaries(&self, id: i64, threshold: f64) -> Result<Vec<Value>> {
        let mut stmt = self
            .db
            .prepare("SELECT json FROM frame_logs WHERE recording_id=?1 ORDER BY id")?;
        let mut rows = stmt.query([id])?;
        let mut result = vec![];
        while let Some(row) = rows.next()? {
            self.check_read()?;
            anyhow::ensure!(
                result.len() < 128,
                "Recording exceeds 128 frame logs; export them instead"
            );
            let raw = row.get_ref(0)?.as_str()?;
            anyhow::ensure!(
                raw.len() <= 128 * 1024 * 1024,
                "Stored frame log exceeds 128 MiB"
            );
            let log: FrameSeries = serde_json::from_str(raw)?;
            result.push(json!({"filename":log.filename,"aligned":log.anchor_ns.is_some(),"anchor_ns":log.anchor_ns,"uncertainty_ms":log.uncertainty_ms,"summary":recording::summarize(&log.frames,threshold)}));
        }
        Ok(result)
    }
    pub fn detail(&self, id: i64, threshold: f64) -> Result<Value> {
        self.check_read()?;
        let recording = self.get(id)?;
        let summaries = self.frame_summaries(id, threshold)?;
        let mut statement = self
            .db
            .prepare("SELECT json FROM samples WHERE recording_id=?1 ORDER BY monotonic_ns")?;
        let mut rows = statement.query([id])?;
        let mut metrics: BTreeMap<String, (f64, u64)> = BTreeMap::new();
        while let Some(row) = rows.next()? {
            self.check_read()?;
            let text: String = row.get(0)?;
            let sample: Snapshot = serde_json::from_str(&text)?;
            for gpu in &sample.gpus {
                for (name, metric) in &gpu.metrics {
                    if let Some(value) = metric.value {
                        let entry = metrics.entry(format!("{}:{name}", gpu.pci)).or_default();
                        entry.0 += value;
                        entry.1 += 1;
                    }
                }
            }
        }
        let averages: BTreeMap<_, _> = metrics
            .into_iter()
            .map(|(key, (sum, count))| (key, sum / count as f64))
            .collect();
        Ok(
            json!({"recording": recording, "frame_logs": summaries, "mean_sampled_gpu_metrics": averages}),
        )
    }
    pub fn compare(&self, ids: &[i64], threshold: f64) -> Result<Value> {
        self.check_read()?;
        if ids.len() < 2 {
            bail!("Select at least two recordings");
        }
        anyhow::ensure!(
            ids.iter().collect::<std::collections::BTreeSet<_>>().len() == ids.len(),
            "Compare requires distinct recording IDs; one recording cannot count as multiple runs"
        );
        let recordings: Vec<_> = ids.iter().map(|&id| self.get(id)).collect::<Result<_>>()?;
        let reference = &recordings[0].metadata["environment"];
        let mut mismatches: Vec<_> = recordings
            .iter()
            .filter(|r| &r.metadata["environment"] != reference)
            .map(|r| {
                format!(
                    "Recording {} has different environment or monitor metadata",
                    r.id
                )
            })
            .collect();
        let signatures: Vec<_> = recordings
            .iter()
            .map(|r| comparison_settings(r, &mut mismatches))
            .collect();
        for (recording, settings) in recordings.iter().zip(&signatures).skip(1) {
            for (key, label) in [
                ("sampling", "sampling intervals or stutter threshold"),
                (
                    "launches",
                    "effective launch settings (including GPU targets)",
                ),
                (
                    "saved_profiles",
                    "saved profile settings; effective launches are unavailable",
                ),
            ] {
                if key == "launches"
                    && (settings["launches_complete"] != true
                        || signatures[0]["launches_complete"] != true)
                {
                    continue;
                }
                if key == "saved_profiles"
                    && (settings[key].is_null() || signatures[0][key].is_null())
                {
                    continue;
                }
                if settings[key] != signatures[0][key] {
                    mismatches.push(format!("Recording {} has different {label} from recording {}. Review intentional condition changes before interpreting results.", recording.id, recordings[0].id));
                }
            }
        }
        let mut groups: BTreeMap<String, Vec<FrameSummary>> = BTreeMap::new();
        let mut details = Vec::new();
        for recording in &recordings {
            let detail = self.detail(recording.id, threshold)?;
            if let Some(logs) = detail["frame_logs"].as_array() {
                if logs.len() > 1 {
                    mismatches.push(format!("Recording {} has multiple frame logs; run variation uses its first log. Inspect all individual summaries.",recording.id));
                }
                if let Some(summary) = logs
                    .first()
                    .and_then(|log| log.get("summary"))
                    .filter(|v| !v.is_null())
                {
                    groups
                        .entry(recording.label.clone())
                        .or_default()
                        .push(serde_json::from_value(summary.clone())?);
                }
            }
            details.push(detail);
        }
        let variation: BTreeMap<_, _> = groups.into_iter().map(|(label, runs)| {
            let fps: Vec<_> = runs.iter().map(|s| s.average_fps).collect();
            let p99: Vec<_> = runs.iter().map(|s| s.p99_ms).collect();
            (label, json!({"runs": runs.len(), "average_fps_range": range(&fps), "p99_ms_range": range(&p99)}))
        }).collect();
        Ok(
            json!({"recordings": details, "mismatch_warnings": mismatches, "run_variation_by_label": variation,
            "interpretation": "Observed differences and ranges; no causal or statistical significance claim. Playback quality is user-reported in notes."}),
        )
    }
    pub fn export(&self, id: i64, destination: &Path) -> Result<()> {
        self.check_read()?;
        let recording = self.get(id)?;
        if !destination.is_absolute() {
            bail!("Export destination must be absolute");
        }
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        fs::DirBuilder::new()
            .mode(0o700)
            .create(destination)
            .context("Export destination must be a new directory")?;
        // Publish the interruption marker durably before any bundle data. A
        // process crash cannot run an error handler to create it afterward.
        crate::fsutil::atomic_private_write(
            &destination.join("INCOMPLETE"),
            b"Export in progress or interrupted; do not treat this bundle as complete.",
        )?;
        fs::File::open(destination)?.sync_all()?;
        fs::File::open(
            destination
                .parent()
                .context("Export needs a parent directory")?,
        )?
        .sync_all()?;
        let result = (|| -> Result<()> {
            write_json(destination.join("metadata.json"), &recording)?;
            let writer = fs::File::create(destination.join("telemetry.jsonl"))?;
            use std::io::Write;
            writer.set_permissions(fs::Permissions::from_mode(0o600))?;
            let mut writer = std::io::BufWriter::with_capacity(64 * 1024, writer);
            let mut statement = self
                .db
                .prepare("SELECT json FROM samples WHERE recording_id=?1 ORDER BY monotonic_ns")?;
            let mut rows = statement.query([id])?;
            while let Some(row) = rows.next()? {
                self.check_read()?;
                let raw: String = row.get(0)?;
                // Stored snapshots were validated on insertion. Preserve the raw
                // row without deserializing the entire recording into memory.
                writer.write_all(raw.as_bytes())?;
                writer.write_all(b"\n")?;
            }
            writer.flush()?;
            writer.get_ref().sync_all()?;
            write_json(destination.join("markers.json"), &recording.markers)?;
            let mut statement = self
                .db
                .prepare("SELECT json FROM frame_logs WHERE recording_id=?1 ORDER BY id")?;
            let mut rows = statement.query([id])?;
            let mut i = 0;
            while let Some(row) = rows.next()? {
                self.check_read()?;
                let text = row.get_ref(0)?.as_str()?;
                anyhow::ensure!(
                    text.len() <= 128 * 1024 * 1024,
                    "Stored frame log exceeds 128 MiB"
                );
                let log: RawFrameExport = serde_json::from_str(text)?;
                crate::fsutil::atomic_private_write(
                    &destination.join(format!("frames-{i}.csv")),
                    log.raw.as_bytes(),
                )?;
                write_json(
                    destination.join(format!("frames-{i}-alignment.json")),
                    &json!({"original_name": log.filename,
                    "anchor_ns": log.anchor_ns, "uncertainty_ms": log.uncertainty_ms}),
                )?;
                i += 1;
            }
            // JSON/CSV helpers sync their files; sync directory entries before
            // removing the marker. Completion requires no per-sample fsync.
            fs::File::open(destination)?.sync_all()?;
            fs::remove_file(destination.join("INCOMPLETE"))?;
            fs::File::open(destination)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            if let Err(error) = crate::fsutil::atomic_private_write(
                &destination.join("INCOMPLETE"),
                b"Export failed; do not treat this bundle as complete.",
            ) {
                crate::logging::event(
                    "warn",
                    "recording.export_marker_failed",
                    crate::logging::error_fields(&error),
                );
            }
        }
        result
    }
}

/// Stable explicit settings only: never compare PID/anchor, verification time,
/// generated launcher text, or launcher presentation as experimental conditions.
fn profile_settings(profile: &Value) -> Value {
    json!({
        "executable":profile["executable"],
        "args":profile.get("args").cloned().unwrap_or_else(||json!([])),
        "gpu":profile["gpu"],
        "vulkan_only":profile.get("vulkan_only").and_then(Value::as_bool).unwrap_or(false),
        "firefox":profile.get("firefox").and_then(Value::as_bool).unwrap_or(false),
        "record":profile.get("record").and_then(Value::as_bool).unwrap_or(false),
        "mangohud":profile.get("mangohud").and_then(Value::as_bool).unwrap_or(false),
        "environment":profile.get("environment").cloned().unwrap_or_else(||json!({})),
        "working_directory":profile["working_directory"],
        "file_argument":profile["desktop"]["file_argument"],
    })
}
fn comparison_settings(recording: &Recording, warnings: &mut Vec<String>) -> Value {
    let metadata = &recording.metadata;
    if metadata["environment"]
        .as_object()
        .is_none_or(|m| m.is_empty())
    {
        warnings.push(format!(
            "Recording {} has missing environment/monitor metadata; comparability is unverified.",
            recording.id
        ));
    }
    let sampling = json!({"normal_interval_ms":metadata["normal_interval_ms"],
        "recording_interval_ms":metadata["recording_interval_ms"],
        "stutter_threshold_ms":metadata["stutter_threshold_ms"]});
    if sampling
        .as_object()
        .is_some_and(|m| m.values().any(Value::is_null))
    {
        warnings.push(format!(
            "Recording {} has missing sampling settings; comparability is unverified.",
            recording.id
        ));
    }
    let mut launches = vec![];
    let mut complete = true;
    if let Some(values) = metadata["launches"].as_array().filter(|v| !v.is_empty()) {
        for launch in values {
            if launch["profile"].is_object() {
                let mut profile = profile_settings(&launch["profile"]);
                // Registered, resolved intent wins over a stored profile alias.
                if let Some(gpu) = launch.get("gpu").filter(|v| v.is_string()) {
                    profile["gpu"] = gpu.clone();
                }
                if profile["executable"].as_str().is_none_or(str::is_empty)
                    || profile["gpu"].as_str().is_none_or(str::is_empty)
                {
                    complete = false;
                }
                launches.push(profile);
            } else {
                complete = false;
            }
        }
    } else {
        complete = false;
    }
    launches.sort_by_cached_key(Value::to_string);
    let saved_profiles = if complete {
        Value::Null
    } else {
        warnings.push(format!("Recording {} has missing effective launch settings; saved profiles alone do not establish what ran.",recording.id));
        let mut saved: Vec<_> = metadata["profiles"]
            .as_array()
            .into_iter()
            .flatten()
            .map(profile_settings)
            .collect();
        saved.sort_by_cached_key(Value::to_string);
        json!(saved)
    };
    json!({"sampling":sampling,"launches":launches,"launches_complete":complete,"saved_profiles":saved_profiles})
}

#[derive(serde::Deserialize)]
struct RawFrameExport {
    filename: String,
    raw: String,
    anchor_ns: Option<u64>,
    uncertainty_ms: Option<f64>,
}

fn range(values: &[f64]) -> Value {
    json!({"min": values.iter().copied().reduce(f64::min), "max": values.iter().copied().reduce(f64::max)})
}
fn write_json(path: impl AsRef<Path>, value: &impl Serialize) -> Result<()> {
    crate::fsutil::atomic_private_write(path.as_ref(), &serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn bounded_json(value: &impl Serialize, limit: usize) -> Result<String> {
    struct Buffer {
        data: Vec<u8>,
        limit: usize,
    }
    impl std::io::Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.data.len()) {
                return Err(std::io::Error::other("Encoded frame log exceeds 128 MiB"));
            }
            self.data.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Buffer {
        data: vec![],
        limit,
    };
    serde_json::to_writer(&mut writer, value)?;
    Ok(String::from_utf8(writer.data)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_marks_handled_failures_and_preserves_existing_destinations() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("db.sqlite"), 16).unwrap();
        let id = store.start("fixture", 1, &json!({})).unwrap();
        store.stop(id, 2, "complete").unwrap();
        // Corrupt fixture log fails after telemetry has been written.
        store
            .db
            .execute(
                "INSERT INTO frame_logs(recording_id,json) VALUES(?1,?2)",
                params![id, "invalid fixture JSON"],
            )
            .unwrap();
        let destination = dir.path().join("failed-export");
        assert!(store.export(id, &destination).is_err());
        assert!(destination.join("metadata.json").exists());
        assert!(destination.join("INCOMPLETE").exists());
        assert_eq!(
            fs::metadata(destination.join("INCOMPLETE"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        store.db.execute("DELETE FROM frame_logs", []).unwrap();
        let contents = fs::read(destination.join("metadata.json")).unwrap();
        assert!(store.export(id, &destination).is_err());
        assert_eq!(
            fs::read(destination.join("metadata.json")).unwrap(),
            contents
        );
        assert!(destination.join("INCOMPLETE").exists());
        let complete = dir.path().join("complete-export");
        store.export(id, &complete).unwrap();
        assert!(!complete.join("INCOMPLETE").exists());
        assert!(complete.join("telemetry.jsonl").exists());
        assert_eq!(
            fs::metadata(&complete).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    #[test]
    fn comparisons_require_distinct_runs_and_check_stable_settings_without_values_in_warnings() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("db.sqlite"), 16).unwrap();
        let profile = json!({"name":"old-name","executable":"/bin/true","gpu":"igpu", "args":["--quality=low"],
            "environment":{"FIXTURE_TOKEN":"private-synthetic-value"},"verification":{"unix_ms":1}});
        let metadata = json!({"environment":{"fixture":"same hardware"},"normal_interval_ms":1000,
            "recording_interval_ms":250,"stutter_threshold_ms":25,
            "profiles":[profile],"launches":[{"process":{"pid":1,"start_ticks":10},"anchor_ns":1,"gpu":"pci-a","profile":profile}]});
        let a = store.start("condition", 1, &metadata).unwrap();
        store.stop(a, 2, "complete").unwrap();
        let mut other = metadata.clone();
        other["launches"][0]["process"] = json!({"pid":55,"start_ticks":900});
        other["launches"][0]["anchor_ns"] = json!(9999);
        other["launches"][0]["profile"]["name"] = json!("renamed");
        other["launches"][0]["profile"]["verification"] = json!({"unix_ms":900});
        let b = store.start("condition", 3, &other).unwrap();
        store.stop(b, 4, "complete").unwrap();
        assert!(store.compare(&[a, a], 25.).is_err());
        assert!(store.compare(&[a, b, a], 25.).is_err());
        assert!(store.compare(&[a, b], 25.).unwrap()["mismatch_warnings"]
            .as_array()
            .unwrap()
            .is_empty());
        for (field, value) in [
            ("args", json!(["--quality=high"])),
            (
                "environment",
                json!({"FIXTURE_TOKEN":"another-private-synthetic-value"}),
            ),
            ("working_directory", json!("/tmp/fixture-work")),
            ("vulkan_only", json!(true)),
            ("firefox", json!(true)),
        ] {
            let mut launches = other["launches"].clone();
            launches[0]["profile"][field] = value;
            store.metadata(b, "launches", launches).unwrap();
            assert!(
                store.compare(&[a, b], 25.).unwrap()["mismatch_warnings"]
                    .to_string()
                    .contains("effective launch settings"),
                "Missing mismatch for {field}"
            );
        }
        // Changing saved but unused profiles cannot falsely report a changed workload.
        store
            .metadata(b, "launches", other["launches"].clone())
            .unwrap();
        store.metadata(b, "profiles", json!([])).unwrap();
        assert!(store.compare(&[a, b], 25.).unwrap()["mismatch_warnings"]
            .as_array()
            .unwrap()
            .is_empty());
        let first = other["launches"][0].clone();
        let mut second = first.clone();
        second["profile"]["executable"] = json!("/bin/printf");
        store
            .metadata(a, "launches", json!([first, second]))
            .unwrap();
        store
            .metadata(b, "launches", json!([second, first]))
            .unwrap();
        assert!(store.compare(&[a, b], 25.).unwrap()["mismatch_warnings"]
            .as_array()
            .unwrap()
            .is_empty());
        store
            .metadata(a, "launches", metadata["launches"].clone())
            .unwrap();
        store
            .metadata(b, "launches", other["launches"].clone())
            .unwrap();
        store
            .metadata(b, "recording_interval_ms", json!(500))
            .unwrap();
        let mut launches = other["launches"].clone();
        launches[0]["profile"]["args"] = json!(["--quality=high"]);
        launches[0]["profile"]["environment"]["FIXTURE_TOKEN"] =
            json!("another-private-synthetic-value");
        launches[0]["gpu"] = json!("pci-b");
        store.metadata(b, "launches", launches).unwrap();
        let warnings = store.compare(&[a, b], 25.).unwrap()["mismatch_warnings"].to_string();
        assert!(
            warnings.contains("sampling intervals")
                && warnings.contains("effective launch settings")
        );
        assert!(!warnings.contains("private-synthetic-value") && !warnings.contains("--quality"));
        store.metadata(b, "launches", json!([])).unwrap();
        let warnings = store.compare(&[a, b], 25.).unwrap()["mismatch_warnings"].to_string();
        assert!(warnings.contains("missing effective launch settings"));
        store.metadata(a, "launches", json!([])).unwrap();
        assert!(store.compare(&[a, b], 25.).unwrap()["mismatch_warnings"]
            .to_string()
            .contains("saved profile settings"));
        let legacy = store.start("legacy", 5, &json!({})).unwrap();
        store.stop(legacy, 6, "complete").unwrap();
        assert!(
            store.compare(&[legacy, b], 25.).unwrap()["mismatch_warnings"]
                .to_string()
                .contains("missing sampling settings")
        );
    }
    #[test]
    fn raw_pages_preserve_duplicate_timestamps_and_thin_timeline_omits_process_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db.sqlite");
        let store = Store::open(&path, 16).unwrap();
        let mut collector = crate::collector::Collector::new(
            dir.path(),
            dir.path(),
            Diagnostics {
                session: String::new(),
                kwin_support: None,
                display_configuration: None,
                versions: Default::default(),
                notes: vec![],
            },
        );
        let mut sample = collector.sample(&crate::config::Config::default());
        sample.monotonic_ns = 10;
        let id = store.start("test", 1, &json!({})).unwrap();
        store.sample(id, &sample).unwrap();
        store.sample(id, &sample).unwrap();
        sample.monotonic_ns = 20;
        store.sample(id, &sample).unwrap();
        assert!(store.samples_page(id, None, None, 1).is_err());
        store.stop(id, 21, "complete").unwrap();
        let first = store.samples_page(id, None, None, 1).unwrap();
        let second = store
            .samples_page(id, first.next_cursor, Some(first.through_ns), 1)
            .unwrap();
        let third = store
            .samples_page(id, second.next_cursor, Some(first.through_ns), 1)
            .unwrap();
        assert_eq!(
            (
                first.samples[0].monotonic_ns,
                second.samples[0].monotonic_ns,
                third.samples[0].monotonic_ns
            ),
            (10, 10, 20)
        );
        assert!(third.next_cursor.is_none());
        let view = store.timeline(id).unwrap();
        assert_eq!(view.samples.len(), 3);
        assert!(serde_json::to_value(&view).unwrap()["samples"][0]
            .get("applications")
            .is_none());
    }
    #[test]
    fn wal_reader_is_query_only_consistent_and_does_not_interrupt_writer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db.sqlite");
        let writer = Store::open(&path, 16).unwrap();
        let id = writer.start("active", 1, &json!({})).unwrap();
        let reader = Store::reader(&path).unwrap();
        assert_eq!(reader.get(id).unwrap().state, "recording");
        assert!(reader.start("forbidden", 2, &json!({})).is_err());
        writer.notes(id, "written during read transaction").unwrap();
        writer.stop(id, 3, "complete").unwrap();
        assert_eq!(reader.get(id).unwrap().state, "recording");
        let fresh = Store::reader(&path).unwrap();
        assert_eq!(fresh.get(id).unwrap().state, "complete");
        assert_eq!(
            fresh.get(id).unwrap().notes,
            "written during read transaction"
        );
        assert!(Store::open(&path, 16).is_err());
        assert!(bounded_json(&json!({"data":"x".repeat(100)}), 20).is_err());
    }
    #[test]
    fn interrupted_recordings_delete_and_export_safety() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db.sqlite");
        let id = {
            let store = Store::open(&path, 16).unwrap();
            let id = store.start("test", 10, &json!({})).unwrap();
            store
                .marker(
                    id,
                    &Marker {
                        monotonic_ns: 11,
                        label: "event".into(),
                    },
                )
                .unwrap();
            assert!(store.delete(id).is_err());
            id
        };
        let store = Store::open(&path, 16).unwrap();
        assert_eq!(store.get(id).unwrap().state, "interrupted");
        let export = dir.path().join("export");
        store.export(id, &export).unwrap();
        assert!(store.export(id, &export).is_err());
        store.delete(id).unwrap();
        assert!(store.get(id).is_err());
    }
    #[test]
    fn exclusive_owner_storage_limit_and_schema_gate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db.sqlite");
        {
            let store = Store::open(&path, 1).unwrap();
            let id = store.start("still-active", 1, &json!({})).unwrap();
            assert!(Store::open(&path, 1).is_err());
            assert_eq!(store.get(id).unwrap().state, "recording");
            assert!(store.notes(id, &"a".repeat(2 * 1024 * 1024)).is_err());
            assert_eq!(store.get(id).unwrap().notes, "");
        }
        {
            let db = Connection::open(&path).unwrap();
            db.pragma_update(None, "user_version", 999).unwrap();
        }
        assert!(Store::open(&path, 1).is_err());
    }
}
