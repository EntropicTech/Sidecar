use crate::model::{Frame, FrameLog};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

pub fn parse_frame_log(
    recording_id: i64,
    filename: String,
    raw: String,
    anchor_ns: Option<u64>,
    uncertainty_ms: Option<f64>,
) -> Result<FrameLog> {
    if raw.len() > 64 * 1024 * 1024 {
        bail!("Frame log exceeds 64 MiB import limit");
    }
    if uncertainty_ms.is_some_and(|n| !n.is_finite() || n < 0.0) {
        bail!("Invalid alignment uncertainty");
    }
    anyhow::ensure!(
        filename.len() <= 512,
        "Frame log filename exceeds 512 bytes"
    );
    let mut offset = 0;
    let start = raw
        .split_inclusive('\n')
        .find_map(|line| {
            let at = offset;
            offset += line.len();
            let mut ft = false;
            let mut elapsed = false;
            for field in line
                .split(',')
                .take(256)
                .map(|s| s.trim().trim_matches('"'))
            {
                ft |= field == "frametime";
                elapsed |= field == "elapsed";
            }
            (ft && elapsed).then_some(at)
        })
        .context("Unsupported MangoHud log: expected named frametime and elapsed columns")?;
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .from_reader(&raw.as_bytes()[start..]);
    let headers = reader.headers()?.clone();
    let ft = headers
        .iter()
        .position(|h| h == "frametime")
        .context("CSV header has no standalone frametime column")?;
    let elapsed = headers
        .iter()
        .position(|h| h == "elapsed")
        .context("CSV header has no standalone elapsed column")?;
    anyhow::ensure!(headers.len() <= 256, "CSV has more than 256 columns");
    anyhow::ensure!(
        headers.iter().filter(|h| *h == "frametime").count() == 1
            && headers.iter().filter(|h| *h == "elapsed").count() == 1,
        "CSV has ambiguous duplicate timing columns"
    );
    let mut frames = Vec::new();
    let mut previous = None;
    for row in reader.records() {
        let row = row.context("Malformed MangoHud CSV row")?;
        let frametime_ms: f64 = row
            .get(ft)
            .context("Missing frametime")?
            .parse()
            .context("Invalid frametime")?;
        let elapsed_ns: u64 = row
            .get(elapsed)
            .context("Missing elapsed")?
            .parse()
            .context("Invalid elapsed timestamp")?;
        if !frametime_ms.is_finite() || frametime_ms <= 0.0 {
            bail!("Frametimes must be finite and positive");
        }
        if previous.is_some_and(|p| elapsed_ns <= p) {
            bail!("Frame timestamps must increase strictly");
        }
        previous = Some(elapsed_ns);
        anyhow::ensure!(
            frames.len() < 1_000_000,
            "Frame log exceeds one million frames"
        );
        frames.push(Frame {
            elapsed_ns,
            frametime_ms,
        });
    }
    if frames.is_empty() {
        bail!("No frame samples in log");
    }
    Ok(FrameLog {
        recording_id,
        filename,
        raw,
        anchor_ns,
        uncertainty_ms,
        frames,
    })
}

/// Display reduction only. Statistics and exports always retain all input frames.
pub fn frame_peaks(frames: &[Frame], points: usize) -> Vec<Frame> {
    if points == 0 {
        return vec![];
    }
    let stride = frames.len().div_ceil(points).max(1);
    frames
        .chunks(stride)
        .filter_map(|chunk| {
            chunk
                .iter()
                .max_by(|a, b| a.frametime_ms.total_cmp(&b.frametime_ms))
        })
        .cloned()
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameSummary {
    pub frames: usize,
    pub average_fps: f64,
    pub median_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub threshold_ms: f64,
    pub frames_over_threshold: usize,
}

pub fn summarize(frames: &[Frame], threshold_ms: f64) -> Option<FrameSummary> {
    if frames.is_empty() || !threshold_ms.is_finite() || threshold_ms <= 0.0 {
        return None;
    }
    let mut times: Vec<_> = frames.iter().map(|f| f.frametime_ms).collect();
    times.sort_by(f64::total_cmp);
    let percentile = |fraction: f64| {
        times[((times.len() as f64 * fraction).ceil() as usize)
            .saturating_sub(1)
            .min(times.len() - 1)]
    };
    Some(FrameSummary {
        frames: frames.len(),
        average_fps: 1000.0 * times.len() as f64 / times.iter().sum::<f64>(),
        median_ms: percentile(0.5),
        p95_ms: percentile(0.95),
        p99_ms: percentile(0.99),
        threshold_ms,
        frames_over_threshold: times.iter().filter(|&&n| n > threshold_ms).count(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentRun {
    pub run: usize,
    pub repetition: usize,
    pub condition: String,
    pub warmup_seconds: u64,
    pub recording_seconds: u64,
}

pub fn experiment_template() -> Vec<ExperimentRun> {
    let conditions = ["browser-idle", "video-dgpu", "video-igpu"];
    (0..3)
        .flat_map(|repetition| {
            (0..3).map(move |position| ExperimentRun {
                run: repetition * 3 + position + 1,
                repetition: repetition + 1,
                condition: conditions[(position + repetition) % 3].into(),
                warmup_seconds: 60,
                recording_seconds: 120,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn display_reduction_is_bounded_and_retains_stutter_peaks() {
        let mut frames: Vec<Frame> = (0..10_000)
            .map(|i| Frame {
                elapsed_ns: i,
                frametime_ms: 10.,
            })
            .collect();
        frames[505].frametime_ms = 500.;
        let points = frame_peaks(&frames, 100);
        assert!(points.len() <= 100);
        assert!(points
            .iter()
            .any(|f| f.elapsed_ns == 505 && f.frametime_ms == 500.));
        assert_eq!(frames.len(), 10_000);
    }
    #[test]
    fn parses_reordered_columns_and_metadata() {
        let log = parse_frame_log(
            1,
            "test.csv".into(),
            "os,cpu\nLinux,Ryzen\nelapsed,extra,frametime\n100,hello,10\n200,world,20\n".into(),
            None,
            None,
        )
        .unwrap();
        assert_eq!(log.frames.len(), 2);
        assert_eq!(log.frames[1].frametime_ms, 20.0);
        let summary = summarize(&log.frames, 15.0).unwrap();
        assert_eq!(summary.frames_over_threshold, 1);
        assert_eq!(summary.p99_ms, 20.0);
        assert!((summary.average_fps - 66.666666).abs() < 0.001);
        assert!(log.anchor_ns.is_none());
    }
    #[test]
    fn rejects_malformed_empty_nan_and_nonmonotonic() {
        for raw in [
            "fps\n60",
            "frametime,elapsed\n",
            "frametime,elapsed\nNaN,1",
            "frametime,elapsed\n0,1",
            "frametime,elapsed\n10,2\n20,1",
            "frametime,elapsed\n10,1,extra",
            "\"frametime,elapsed\"\n10\n",
        ] {
            assert!(
                parse_frame_log(1, "test".into(), raw.into(), None, None).is_err(),
                "{raw}"
            );
        }
    }
    #[test]
    fn rotates_conditions_evenly() {
        let runs = experiment_template();
        assert_eq!(runs.len(), 9);
        assert_eq!(runs[0].condition, runs[5].condition);
        assert_eq!(
            runs.iter().filter(|r| r.condition == "video-igpu").count(),
            3
        );
    }
}
