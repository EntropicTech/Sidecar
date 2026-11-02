//! Route live reads independently of the recording writer and bounded read jobs.
use super::*;
impl Service {
    pub async fn dispatch(&self, operation: Operation, value: Value) -> Result<String> {
        if operation == Operation::Capabilities {
            return Ok(json!({"api_version":igpu_core::API_VERSION,"build":igpu_core::build::metadata(),"features":["guarded_stop","request_ids","pending_trials","structured_logging","isolated_recording_io","compact_recording_timeline","coordinated_updates"]}).to_string());
        }
        if operation.live() {
            let view = self
                .live
                .lock()
                .map_err(|_| anyhow::anyhow!("Live state poisoned"))?
                .view();

            return tokio::task::spawn_blocking(move || {
                let result = if operation == Operation::VerifyTrial {
                    let process = serde_json::from_value(value["process"].clone())?;
                    serde_json::to_value(view.verify(&process))?
                } else {
                    view.request(operation)?
                };
                encode(&result)
            })
            .await
            .map_err(|e| task_failure(e, "Live read"))?;
        }
        if operation.read_job() {
            let permit = self
                .jobs
                .clone()
                .try_acquire_owned()
                .context("Recording reads busy; retry after the current operation finishes")?;
            let path = self.read_path.clone();

            let threshold = self
                .live
                .lock()
                .map_err(|_| anyhow::anyhow!("Live state poisoned"))?
                .config
                .stutter_threshold_ms;
            return tokio::task::spawn_blocking(move || -> Result<String> {
                let _permit = permit;
                if operation == Operation::Launchers {
                    return encode(&serde_json::to_value(launchers::installed())?);
                }
                if operation == Operation::Experiment {
                    return encode(&serde_json::to_value(recording::experiment_template())?);
                }
                let store = Store::reader(&path)?;
                let id = || value["id"].as_i64().context("Missing recording id");
                let result = match operation {
                    Operation::Recordings => serde_json::to_value(store.list()?)?,
                    Operation::Detail => store.detail(id()?, threshold)?,
                    Operation::Samples => serde_json::to_value(store.samples(id()?)?)?,
                    Operation::SamplesPage => serde_json::to_value(store.samples_page(
                        id()?,
                        if value["after"].is_null() {
                            None
                        } else {
                            Some(serde_json::from_value(value["after"].clone())?)
                        },
                        if value["through_ns"].is_null() {
                            None
                        } else {
                            Some(
                                value["through_ns"]
                                    .as_u64()
                                    .context("through_ns must be an unsigned integer")?,
                            )
                        },
                        if value["limit"].is_null() {
                            16
                        } else {
                            value["limit"]
                                .as_u64()
                                .context("limit must be an unsigned integer")?
                                as usize
                        },
                    )?)?,
                    Operation::Timeline => serde_json::to_value(store.timeline(id()?)?)?,
                    Operation::Frames => serde_json::to_value(store.frame_logs(id()?)?)?,
                    Operation::Compare => {
                        let ids: Vec<i64> = serde_json::from_value(value["ids"].clone())?;
                        anyhow::ensure!(ids.len() <= 32, "Compare supports at most 32 recordings");
                        store.compare(&ids, threshold)?
                    }
                    Operation::Export => {
                        store.export(
                            id()?,
                            &PathBuf::from(
                                value["destination"]
                                    .as_str()
                                    .context("Missing destination")?,
                            ),
                        )?;
                        json!(true)
                    }
                    _ => bail!("Unknown read operation"),
                };
                encode(&result)
            })
            .await
            .map_err(|e| task_failure(e, "Recording read"))?;
        }
        let permit = self
            .writes
            .clone()
            .try_acquire_owned()
            .context("Recording writer busy; retry after the current operation finishes")?;
        let result = self.writer.request(&self.live, operation, value).await?;
        drop(permit);
        // Serialization is outside every sampler lock and off the async executor.
        tokio::task::spawn_blocking(move || encode(&result))
            .await
            .map_err(|e| task_failure(e, "Response encoding"))?
    }
}
fn encode(value: &Value) -> Result<String> {
    let text = serde_json::to_string(value)?;
    anyhow::ensure!(
        text.len() <= 70 * 1024 * 1024,
        "Response exceeds 70 MiB; use an export instead"
    );
    Ok(text)
}
