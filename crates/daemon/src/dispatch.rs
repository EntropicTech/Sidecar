//! Admit requests before parsing; retain permits through blocking work.
use super::*;
impl Service {
    pub async fn dispatch(
        &self,
        operation: Operation,
        value: Value,
        permit: tokio::sync::OwnedSemaphorePermit,
    ) -> Result<String> {
        if operation == Operation::Capabilities {
            return Ok(json!({"api_version":igpu_core::API_VERSION,"build":igpu_core::build::metadata(),"features":["guarded_stop","request_ids","pending_trials","structured_logging","isolated_recording_io","compact_recording_timeline","coordinated_updates","launch_path_audit","cached_software_versions","normal_launches","simple_app_routing"]}).to_string());
        }
        if operation.live() {
            let view = self
                .live
                .lock()
                .map_err(|_| anyhow::anyhow!("Live state poisoned"))?
                .view();

            return tokio::task::spawn_blocking(move || {
                let _permit = permit;
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
            let path = self.read_path.clone();

            let normal_config = if matches!(
                operation,
                Operation::NormalLaunchPreview
                    | Operation::NormalLaunchStatus
                    | Operation::AppRoutePreview
            ) {
                Some(
                    self.live
                        .lock()
                        .map_err(|_| anyhow::anyhow!("Live state poisoned"))?
                        .config
                        .clone(),
                )
            } else {
                None
            };
            let app_gpus = if operation == Operation::AppRoutePreview {
                self.live
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Live state poisoned"))?
                    .snapshot
                    .gpus
                    .clone()
            } else {
                Vec::new()
            };
            let threshold = self
                .live
                .lock()
                .map_err(|_| anyhow::anyhow!("Live state poisoned"))?
                .config
                .stutter_threshold_ms;
            return tokio::task::spawn_blocking(move || -> Result<String> {
                let _permit = permit;
                if operation==Operation::AppRoutePreview {
                    let gpus:Vec<_>=app_gpus.iter().filter(|g|g.integrated).collect();
                    anyhow::ensure!(gpus.len()==1,"A single iGPU is required; use Advanced settings for a custom GPU selection");
                    let setup=serde_json::from_value(value)?;
                    return encode(&serde_json::to_value(igpu_core::normal_launch::app_preview(&Paths::discover()?, &normal_config.context("App configuration unavailable")?, &setup, &gpus[0].pci)?)?);
                }
                if operation == Operation::NormalLaunchStatus {
                    return encode(&serde_json::to_value(
                        igpu_core::normal_launch::statuses_for(
                            &Paths::discover()?,
                            normal_config
                                .as_ref()
                                .context("Missing normal launch configuration")?,
                        )?,
                    )?);
                }
                if operation == Operation::NormalLaunchPreview {
                    let setup = serde_json::from_value(value)?;
                    return encode(&serde_json::to_value(igpu_core::normal_launch::preview(
                        &Paths::discover()?,
                        &normal_config.context("Missing normal launch configuration")?,
                        &setup,
                    )?)?);
                }
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
        let result = self.writer.request(&self.live, operation, value).await?;
        // Serialization is outside every sampler lock and off the async executor.
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            encode(&result)
        })
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

/// Independent classes reserve capacity for controls during slow reads/live work.
pub(super) struct Admission {
    live: Arc<tokio::sync::Semaphore>,
    reads: Arc<tokio::sync::Semaphore>,
    writes: Arc<tokio::sync::Semaphore>,
}
impl Admission {
    pub(super) fn new() -> Self {
        Self {
            live: Arc::new(tokio::sync::Semaphore::new(4)),
            reads: Arc::new(tokio::sync::Semaphore::new(2)),
            writes: Arc::new(tokio::sync::Semaphore::new(8)),
        }
    }
    pub(super) async fn parse(
        &self,
        operation: Operation,
        payload: &str,
    ) -> zbus::fdo::Result<(Value, tokio::sync::OwnedSemaphorePermit)> {
        let permit = self.acquire(operation)?;
        let payload = payload.to_owned();
        let (value, permit) =
            tokio::task::spawn_blocking(move || (serde_json::from_str::<Value>(&payload), permit))
                .await
                .map_err(|e| {
                    zbus::fdo::Error::Failed(task_failure(e, "Request parsing").to_string())
                })?;
        let value = value.map_err(|_| {
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
        Ok((value, permit))
    }
    pub(super) fn acquire(
        &self,
        operation: Operation,
    ) -> zbus::fdo::Result<tokio::sync::OwnedSemaphorePermit> {
        let (slots, message) = if operation.live() || operation == Operation::Capabilities {
            (
                &self.live,
                "Live reads busy; retry after the current operation finishes",
            )
        } else if operation.read_job() {
            (
                &self.reads,
                "Recording reads busy; retry after the current operation finishes",
            )
        } else {
            (
                &self.writes,
                "Recording writer busy; retry after the current operation finishes",
            )
        };
        slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| zbus::fdo::Error::LimitsExceeded(message.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn admission_is_bounded_per_class_and_released_after_errors() {
        let admission = Admission::new();
        let live: Vec<_> = (0..4)
            .map(|_| admission.acquire(Operation::Snapshot).unwrap())
            .collect();
        assert!(admission.acquire(Operation::Candidates).is_err());
        assert!(admission.acquire(Operation::Capabilities).is_err());
        let reads: Vec<_> = (0..2)
            .map(|_| admission.acquire(Operation::Export).unwrap())
            .collect();
        assert!(admission.acquire(Operation::Timeline).is_err());
        let writes: Vec<_> = (0..8)
            .map(|_| admission.acquire(Operation::Mark).unwrap())
            .collect();
        assert!(admission.acquire(Operation::SaveProfile).is_err());
        drop(live);
        drop(reads);
        drop(writes);
        for operation in Operation::ALL {
            assert!(admission.acquire(*operation).is_ok());
        }
    }
    #[tokio::test]
    async fn reject_busy_before_parsing_and_release_parse_failures() {
        let admission = Admission::new();
        let held: Vec<_> = (0..4)
            .map(|_| admission.acquire(Operation::Snapshot).unwrap())
            .collect();
        assert!(matches!(
            admission.parse(Operation::Snapshot, "invalid").await,
            Err(zbus::fdo::Error::LimitsExceeded(_))
        ));
        drop(held);
        for _ in 0..8 {
            for invalid in ["invalid", "[]"] {
                assert!(matches!(
                    admission.parse(Operation::Snapshot, invalid).await,
                    Err(zbus::fdo::Error::InvalidArgs(_))
                ));
            }
            assert!(admission.parse(Operation::Snapshot, "{}").await.is_ok());
        }
    }
    #[tokio::test]
    async fn cancelled_reader_retains_admission_until_blocking_job_finishes() {
        let admission = Admission::new();
        let held: Vec<_> = (0..3)
            .map(|_| admission.acquire(Operation::Snapshot).unwrap())
            .collect();
        let permit = admission.acquire(Operation::Snapshot).unwrap();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finish_tx, finish_rx) = std::sync::mpsc::channel();
        let job = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            started_tx.send(()).unwrap();
            finish_rx.recv().unwrap();
        });
        started_rx.await.unwrap();
        job.abort();
        assert!(admission.acquire(Operation::Snapshot).is_err());
        finish_tx.send(()).unwrap();
        job.await.unwrap();
        assert!(admission.acquire(Operation::Snapshot).is_ok());
        drop(held);
    }
}
