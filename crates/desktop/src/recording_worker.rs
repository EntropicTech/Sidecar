//! Bounded recording I/O, separate from telemetry and window control.
use super::*;
pub enum Job {
    Request {
        method: Operation,
        value: Value,
        generation: u64,
    },
    Import {
        id: i64,
        path: std::path::PathBuf,
        anchor: Option<u64>,
    },
    Poll,
}
pub fn spawn(
    shared: Arc<Mutex<Backend>>,
    context: lifecycle::Control,
) -> (mpsc::SyncSender<Job>, worker_health::Health) {
    let (sender, receiver) = mpsc::sync_channel(2);
    let health = worker_health::spawn("recordings", move || {
        #[cfg(feature = "screenshot")]
        match std::env::var("IGPU_MAN_SCREENSHOT_WORKER_FAILURE").as_deref() {
            Ok("recordings") => panic!("synthetic recording worker failure"),
            Ok("poison") => {
                let _guard = shared.lock().unwrap();
                panic!("synthetic poisoned backend");
            }
            _ => {}
        }
        for job in receiver {
            let (method, value, generation) = match job {
                Job::Request {
                    method,
                    value,
                    generation,
                } => (method, Ok(value), Some(generation)),
                Job::Import { id, path, anchor } => (
                    Operation::Import,
                    (|| -> anyhow::Result<Value> {
                        Ok(
                            json!({"id":id,"filename":path.file_name().unwrap_or_default().to_string_lossy(),"raw":igpu_core::fsutil::read_text_limited(&path,64*1024*1024)?,"anchor_ns":anchor}),
                        )
                    })(),
                    None,
                ),
                Job::Poll => (Operation::Recordings, Ok(json!({})), None),
            };
            if method == Operation::Detail
                && generation != Some(shared.lock().unwrap().recording_generation)
            {
                continue;
            }
            if method != Operation::Recordings {
                shared.lock().unwrap().recording_busy = true;
            }
            let result = value.and_then(|value| {
                client::call::<Value>(method.as_str(), &value).map(|result| (value, result))
            });
            if method == Operation::Detail
                && generation != Some(shared.lock().unwrap().recording_generation)
            {
                shared.lock().unwrap().recording_busy = false;
                continue;
            }
            let timeline = if method == Operation::Detail {
                result
                    .as_ref()
                    .ok()
                    .map(|(value, _)| client::recording_timeline(value))
            } else {
                None
            };
            let parsed_recordings = if method == Operation::Recordings {
                result
                    .as_ref()
                    .ok()
                    .map(|(_, v)| serde_json::from_value::<Vec<Recording>>(v.clone()))
            } else {
                None
            };
            let mut state = shared.lock().unwrap();
            if method != Operation::Recordings {
                state.recording_busy = false;
            }
            if method == Operation::Detail && generation != Some(state.recording_generation) {
                continue;
            }
            match result {
                Ok((_, value)) => {
                    backend_recovered(&mut state, "recording operations");
                    match method {
                        Operation::Detail => state.detail = Some(Arc::new(value)),
                        Operation::Compare => state.comparison = Some(Arc::new(value)),
                        Operation::Recordings => match parsed_recordings
                            .expect("recordings response was parsed")
                        {
                            Ok(recordings) => state.recordings = Arc::new(recordings),
                            Err(error) => {
                                backend_failure(&mut state, "recording operations", &error.into())
                            }
                        },
                        _ => {}
                    }
                    if method != Operation::Recordings {
                        state.result = format!("Recording operation {method} completed.");
                        state.error.clear();
                    }
                }
                Err(error) => {
                    backend_failure(&mut state, "recording operations", &error);
                    state.error = igpu_core::logging::diagnostic_message(&error);
                }
            }
            if let Some(timeline) = timeline {
                match timeline {
                    Ok(t) => state.apply_timeline(Ok(t.samples), Ok(t.frames)),
                    Err(e) => state.apply_timeline(Err(e), Ok(vec![])),
                }
            }

            drop(state);
            context.request_repaint();
        }
    });
    (sender, health)
}
pub fn operation(method: &str) -> Option<Operation> {
    Operation::parse(method).ok().filter(|op| op.recording())
}
