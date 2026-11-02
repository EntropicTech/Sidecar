//! A bounded FIFO isolates the single recording/configuration writer from sampling.
use super::*;
use tokio::sync::oneshot;

const CAPACITY: usize = 128;
pub enum Work {
    Request {
        operation: Operation,
        value: Value,
        reply: oneshot::Sender<Result<Value>>,
    },
    Sample(Arc<Snapshot>),
    Shutdown(oneshot::Sender<Result<()>>),
}
#[derive(Clone)]
pub struct Writer(pub tokio::sync::mpsc::Sender<Work>);
impl Writer {
    pub fn spawn(mut state: State) -> (Self, oneshot::Receiver<Result<()>>) {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(CAPACITY);
        let (done, completion) = oneshot::channel();
        std::thread::spawn(move || {
            let outcome = (|| -> Result<()> {
                while let Some(work) = receiver.blocking_recv() {
                    match work {
                        Work::Request {
                            operation,
                            value,
                            reply,
                        } => {
                            let result = state.request(operation, value);
                            {
                                let mut live = state
                                    .live
                                    .lock()
                                    .map_err(|_| anyhow::anyhow!("Live state poisoned"))?;
                                live.config = state.config.clone();
                                live.health = state.health.clone();
                            }
                            let _ = reply.send(result); // Caller may have timed out; never retry the mutation.
                        }
                        Work::Sample(sample) => state.persist_sample(&sample)?,
                        Work::Shutdown(reply) => {
                            let result = if state.active.is_some() {
                                state.stop("interrupted").map(|_| ())
                            } else {
                                Ok(())
                            };
                            let failed = result.is_err();
                            let _ = reply.send(result);
                            if failed {
                                bail!("Recording shutdown failed");
                            }
                            return Ok(());
                        }
                    }
                }
                bail!("Recording worker queue disconnected unexpectedly")
            })();
            let _ = done.send(outcome);
        });
        (Self(sender), completion)
    }
    pub fn sample(&self, sample: Arc<Snapshot>) -> Result<()> {
        // Preserve control capacity. Overflow is a visible fatal recording failure,
        // never an unmarked hole in a recording or an unbounded RAM queue.
        anyhow::ensure!(
            self.0.capacity() > 8,
            "Recording writer fell behind; sample queue exhausted"
        );
        self.0
            .try_send(Work::Sample(sample))
            .map_err(|_| anyhow::anyhow!("Recording writer unavailable or busy"))
    }
    pub async fn request(
        &self,
        live: &Arc<Mutex<live::Live>>,
        operation: Operation,
        value: Value,
    ) -> Result<Value> {
        let (reply, result) = oneshot::channel();
        {
            let mut live = live
                .lock()
                .map_err(|_| anyhow::anyhow!("Live state poisoned"))?;
            live.check_update(operation, value["_update_token"].as_str())?;
            if operation == Operation::Stop {
                let id = live.active.context("No recording is active")?;
                if !value["expected_id"].is_null() {
                    anyhow::ensure!(
                        value["expected_id"]
                            .as_i64()
                            .context("expected_id must be an integer")?
                            == id,
                        "Active recording changed; refusing to stop a different recording"
                    );
                }
                let mut value = value;
                value["expected_id"] = json!(id);
                self.0
                    .try_send(Work::Request {
                        operation,
                        value,
                        reply,
                    })
                    .map_err(|_| {
                        anyhow::anyhow!("Recording writer busy; request was not queued")
                    })?;
                // Samples and Stop are enqueued under this same lock, so Stop
                // acknowledges only after every preceding sample was persisted.
                live.active = None;
                live.stopping = Some(id);
            } else {
                self.0
                    .try_send(Work::Request {
                        operation,
                        value,
                        reply,
                    })
                    .map_err(|_| {
                        anyhow::anyhow!("Recording writer busy; request was not queued")
                    })?;
            }
        }
        result
            .await
            .context("Recording worker failed before replying")?
    }
    pub async fn shutdown(&self) -> Result<()> {
        let (reply, result) = oneshot::channel();
        self.0
            .send(Work::Shutdown(reply))
            .await
            .context("Recording worker unavailable during shutdown")?;
        result
            .await
            .context("Recording worker failed during shutdown")?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(dir: &std::path::Path) -> State {
        let config = Config::default();
        let mut collector = Collector::new(
            dir,
            dir,
            Diagnostics {
                session: String::new(),
                kwin_support: None,
                display_configuration: None,
                versions: Default::default(),
                notes: vec![],
            },
        );
        let snapshot = Arc::new(collector.sample(&config));
        let live = Arc::new(Mutex::new(live::Live {
            collector,
            config: config.clone(),
            snapshot,
            ring: VecDeque::new(),
            active: None,
            stopping: None,
            wake: Arc::new(tokio::sync::Notify::new()),
            health: vec![],
            update_lease: None,
        }));
        State {
            live,
            config,
            paths: Paths {
                config: dir.join("config.toml"),
                data: dir.into(),
                applications: dir.join("applications"),
            },
            store: Store::open(&dir.join("db.sqlite"), 16).unwrap(),
            active: None,
            deadline: None,
            last_saved_ns: 0,
            writer_delayed: false,
            health: vec![],
            imported: BTreeSet::new(),
            frame_anchors: Default::default(),
            managed_directories: Default::default(),
        }
    }
    #[tokio::test]
    async fn stop_is_a_fifo_barrier_and_wrong_ids_leave_capture_running() {
        let dir = tempfile::tempdir().unwrap();
        let state = fixture(dir.path());
        let live = state.live.clone();
        let (writer, completion) = Writer::spawn(state);
        let id = writer
            .request(&live, Operation::Start, json!({"label":"test"}))
            .await
            .unwrap()
            .as_i64()
            .unwrap();
        for _ in 0..2 {
            tokio::time::sleep(Duration::from_millis(2)).await;
            let mut live = live.lock().unwrap();
            let mut sample = (*live.snapshot).clone();
            sample.monotonic_ns = monotonic_ns();
            sample.active_recording = Some(id);
            let sample = Arc::new(sample);
            live.snapshot = sample.clone();
            writer.sample(sample).unwrap();
        }
        assert!(writer
            .request(&live, Operation::Stop, json!({"expected_id":id+1}))
            .await
            .is_err());
        assert_eq!(live.lock().unwrap().active, Some(id));
        assert_eq!(
            writer
                .request(&live, Operation::Stop, json!({"expected_id":id}))
                .await
                .unwrap(),
            json!(id)
        );
        let recording = Store::reader(&dir.path().join("db.sqlite"))
            .unwrap()
            .get(id)
            .unwrap();
        assert_eq!(recording.samples, 3);
        assert_eq!(recording.state, "complete");
        assert_eq!(live.lock().unwrap().active, None);
        writer.shutdown().await.unwrap();
        completion.await.unwrap().unwrap();
    }
    #[tokio::test]
    async fn bounded_samples_reserve_capacity_for_stop() {
        let dir = tempfile::tempdir().unwrap();
        let state = fixture(dir.path());
        let live = state.live.clone();
        live.lock().unwrap().active = Some(7);
        let (sender, mut receiver) = tokio::sync::mpsc::channel(CAPACITY);
        let writer = Writer(sender);
        let sample = live.lock().unwrap().snapshot.clone();
        for _ in 0..CAPACITY - 8 {
            writer.sample(sample.clone()).unwrap();
        }
        assert!(writer.sample(sample).is_err());
        let stop = writer.request(&live, Operation::Stop, json!({"expected_id":7}));
        tokio::pin!(stop);
        tokio::select! {r=&mut stop=>panic!("Stop returned before persistence barrier: {r:?}"),_=tokio::time::sleep(Duration::from_millis(5))=>{}}
        assert!(live.lock().unwrap().active.is_none());
        for _ in 0..CAPACITY - 8 {
            assert!(matches!(receiver.recv().await, Some(Work::Sample(_))));
        }
        match receiver.recv().await.unwrap() {
            Work::Request {
                reply, operation, ..
            } => {
                assert_eq!(operation, Operation::Stop);
                reply.send(Ok(json!(7))).unwrap();
            }
            _ => panic!("missing Stop barrier"),
        }
        assert_eq!(stop.await.unwrap(), json!(7));
    }
}
