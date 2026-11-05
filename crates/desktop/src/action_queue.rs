//! Bound explicit dashboard work independently of the native lifecycle channel.
use super::*;

pub(super) fn channel() -> (mpsc::SyncSender<Action>, mpsc::Receiver<Action>) {
    mpsc::sync_channel(8)
}

/// Admission is nonblocking. Rejected work was never dispatched, so only its
/// pending UI state is unwound; accepted mutations are never retried automatically.
pub(super) fn submit(
    sender: &mpsc::SyncSender<Action>,
    action: Action,
    state: &mut Backend,
) -> bool {
    let (action, stopped) = match sender.try_send(action) {
        Ok(()) => return true,
        Err(mpsc::TrySendError::Full(action)) => (action, false),
        Err(mpsc::TrySendError::Disconnected(action)) => (action, true),
    };
    let operation = match action {
        Action::Rpc(method, _) => {
            if method.starts_with("normal_launch_") || method.starts_with("app_route_") {
                state.normal_busy = false;
            }
            if method == "keep_trial" {
                state.trial_keep_pending = false;
            }
            method
        }
        Action::StartTrial { .. } => {
            state.trial_pending = false;
            "start_trial"
        }
        Action::RefreshLaunchers => "refresh_launchers",
        _ => "control_hint",
    };
    state.error = if stopped {
        "The background worker stopped; restart Sidecar and inspect its logs."
    } else {
        "Sidecar is busy; this action was not queued. Wait for the current work to finish before retrying."
    }.into();
    igpu_core::logging::event(
        if stopped { "error" } else { "warn" },
        if stopped {
            "dashboard.worker_unavailable"
        } else {
            "dashboard.action_rejected"
        },
        json!({"operation":operation,"reason":if stopped {"stopped"} else {"busy"}}),
    );
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saturation_preserves_accepted_work_and_unwinds_rejected_state() {
        let (sender, receiver) = channel();
        let mut state = Backend::default();
        // Fill dynamically, then prove admitted mutations retain FIFO order.
        let mut admitted = 0;
        while sender
            .try_send(Action::Rpc("save_profile", json!({"index":admitted})))
            .is_ok()
        {
            admitted += 1;
            assert!(admitted < 100);
        }
        state.normal_busy = true;
        assert!(!submit(
            &sender,
            Action::Rpc("normal_launch_apply", json!({})),
            &mut state
        ));
        assert!(!state.normal_busy && state.error.contains("not queued"));
        state.trial_keep_pending = true;
        assert!(!submit(
            &sender,
            Action::Rpc("keep_trial", json!({})),
            &mut state
        ));
        assert!(!state.trial_keep_pending);
        state.trial_pending = true;
        assert!(!submit(
            &sender,
            Action::StartTrial {
                profile: Box::default(),
                previous: vec![]
            },
            &mut state
        ));
        assert!(!state.trial_pending);
        // Close/Quit control remains independent even when work admission is full.
        let (control, events) = lifecycle::tests::fixture();
        assert!(control.send(Action::Quit));
        assert!(matches!(events.try_recv(), Ok(Action::Quit)));
        for index in 0..admitted {
            assert!(
                matches!(receiver.try_recv(), Ok(Action::Rpc("save_profile", value)) if value["index"] == index)
            );
        }
        assert!(submit(&sender, Action::RefreshLaunchers, &mut state));
        assert!(matches!(receiver.try_recv(), Ok(Action::RefreshLaunchers)));
        drop(receiver);
        assert!(!submit(&sender, Action::RefreshLaunchers, &mut state));
        assert!(state.error.contains("stopped"));
    }
}
