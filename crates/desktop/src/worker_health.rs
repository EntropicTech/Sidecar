//! Observe worker failure without retry loops or recovering poisoned state.
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc,
};
#[derive(Clone)]
pub struct Health(Arc<AtomicU8>);
impl Health {
    pub fn failed(&self) -> bool {
        matches!(self.0.load(Ordering::Acquire), 2 | 3)
    }
    pub fn label(&self) -> &'static str {
        match self.0.load(Ordering::Acquire) {
            0 => "starting",
            1 => "running",
            2 => "failed",
            _ => "stopped",
        }
    }
}
struct Guard {
    name: &'static str,
    health: Health,
}
impl Drop for Guard {
    fn drop(&mut self) {
        let panicking = std::thread::panicking();
        self.health
            .0
            .store(if panicking { 2 } else { 3 }, Ordering::Release);
        igpu_core::logging::event(
            if panicking { "error" } else { "info" },
            if panicking {
                "dashboard.worker_failed"
            } else {
                "dashboard.worker_stopped"
            },
            serde_json::json!({"worker":self.name}),
        );
    }
}
pub fn spawn(name: &'static str, run: impl FnOnce() + Send + 'static) -> Health {
    let health = Health(Arc::new(AtomicU8::new(0)));
    let worker = health.clone();
    if let Err(error) = std::thread::Builder::new()
        .name(format!("igpu-{name}"))
        .spawn(move || {
            worker.0.store(1, Ordering::Release);
            let _guard = Guard {
                name,
                health: worker,
            };
            run();
        })
    {
        health.0.store(2, Ordering::Release);
        igpu_core::logging::event(
            "error",
            "dashboard.worker_start_failed",
            igpu_core::logging::error_fields(&error.into()),
        );
    }
    health
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failures_are_visible_without_resuming_poisoned_state() {
        let state = Arc::new(std::sync::Mutex::new(0));
        let worker_state = state.clone();
        let health = spawn("fixture", move || {
            let _guard = worker_state.lock().unwrap();
            panic!("synthetic worker failure");
        });
        for _ in 0..100 {
            if health.failed() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(health.failed());
        assert!(state.is_poisoned());
    }
}
