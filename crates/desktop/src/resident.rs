//! Keep the tray/model resident while destroying and recreating native windows.
//! Wayland set_visible(false) is unsupported; reuse eframe's on-demand event loop.
use super::*;

struct WindowSession {
    dashboard: Option<Dashboard>,
    parked: Arc<Mutex<Option<Dashboard>>>,
}
impl eframe::App for WindowSession {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if let Some(dashboard) = &mut self.dashboard {
            dashboard.update(ctx, frame);
        }
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(mut dashboard) = self.dashboard.take() {
            dashboard.visible = false;
            dashboard
                .visible_flag
                .store(false, std::sync::atomic::Ordering::Release);
            dashboard.control.parked();
            match self.parked.lock() {
                Ok(mut slot) => *slot = Some(dashboard),
                Err(_) => igpu_core::logging::event("error", "dashboard.park_failed", json!({})),
            }
        }
    }
}

pub fn run(
    control: lifecycle::Control,
    events: mpsc::Receiver<Action>,
    window_icon: egui::IconData,
    tray_icons: Vec<ksni::Icon>,
    start_in_tray: bool,
) -> eframe::Result<()> {
    // Reuse eframe's run-and-return event loop. A parked dashboard owns no
    // mapped window/GL renderer, but keeps drafts, workers and the tray alive.
    let parked = Arc::new(Mutex::new(None::<Dashboard>));
    let mut dashboard = Dashboard::new(control.clone(), events, tray_icons, !start_in_tray);
    if start_in_tray && !wait_for_initial_show(&mut dashboard) {
        return Ok(());
    }
    let mut initial = Some(dashboard);
    loop {
        let slot = parked.clone();
        let first = initial.take();
        eframe::run_native(
            branding::NAME,
            eframe::NativeOptions {
                run_and_return: true,
                renderer: eframe::Renderer::Glow,
                viewport: egui::ViewportBuilder::default()
                    .with_inner_size([1120.0, 780.0])
                    .with_app_id(branding::APP_ID)
                    .with_icon(window_icon.clone()),
                ..Default::default()
            },
            Box::new(move |cc| {
                let dashboard = if let Some(mut dashboard) = first {
                    dashboard.visible = true;
                    dashboard
                        .visible_flag
                        .store(true, std::sync::atomic::Ordering::Release);
                    theme::apply(&cc.egui_ctx);
                    dashboard.control.attach(cc.egui_ctx.clone());
                    dashboard
                } else {
                    let mut dashboard = slot
                        .lock()
                        .map_err(|_| "Parked dashboard poisoned")?
                        .take()
                        .ok_or("Parked dashboard missing")?;
                    dashboard.closing = false;
                    dashboard.visible = true;
                    dashboard
                        .visible_flag
                        .store(true, std::sync::atomic::Ordering::Release);
                    theme::apply(&cc.egui_ctx);
                    dashboard.control.attach(cc.egui_ctx.clone());
                    igpu_core::logging::event("info", "dashboard.window_reopened", json!({}));
                    dashboard
                };
                Ok(Box::new(WindowSession {
                    dashboard: Some(dashboard),
                    parked: slot,
                }))
            }),
        )?;
        let mut guard = parked
            .lock()
            .map_err(|_| eframe::Error::AppCreation("Parked dashboard poisoned".into()))?;
        let dashboard = guard
            .as_mut()
            .ok_or_else(|| eframe::Error::AppCreation("Dashboard did not park".into()))?;
        if dashboard.quitting
            || control.finish_close(
                dashboard
                    .tray_available
                    .load(std::sync::atomic::Ordering::Acquire),
            )
        {
            return Ok(());
        }
        igpu_core::logging::event("info", "dashboard.parked", json!({}));
        while !control.wants_open() {
            match dashboard.ui_events.recv() {
                Ok(Action::Quit) | Err(_) => return Ok(()),
                Ok(action @ (Action::Show | Action::ShowApplications)) => {
                    if matches!(action, Action::ShowApplications) {
                        dashboard.tab = 1;
                    }
                    control.show_requested();
                }
                Ok(Action::Close) => control.hide_requested(),
                Ok(Action::TrialKept) => dashboard.trial_setup = None,
                Ok(_) => {}
            }
        }
        drop(guard);
    }
}

/// Boot grace is bounded and event driven; no native event loop/GL context exists.
fn wait_for_initial_show(dashboard: &mut Dashboard) -> bool {
    let deadline = Instant::now() + Duration::from_secs(15);
    igpu_core::logging::event("info", "dashboard.tray_start_requested", json!({}));
    loop {
        let available = dashboard
            .tray_available
            .load(std::sync::atomic::Ordering::Acquire);
        dashboard.control.tray_state(available);
        dashboard.control.worker_health(
            dashboard.telemetry_health.label(),
            dashboard.recording_health.label(),
            !dashboard.backend.is_poisoned(),
        );
        if dashboard.control.finish_close(true) {
            return false;
        }
        if dashboard.control.wants_open() {
            return true;
        }
        let event = if available {
            dashboard
                .ui_events
                .recv()
                .map_err(|_| mpsc::RecvTimeoutError::Disconnected)
        } else {
            dashboard
                .ui_events
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        };
        match event {
            Ok(Action::Quit) | Err(mpsc::RecvTimeoutError::Disconnected) => return false,
            Ok(action @ (Action::Show | Action::ShowApplications)) => {
                if matches!(action, Action::ShowApplications) {
                    dashboard.tab = 1;
                }
                dashboard.control.show_requested();
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                igpu_core::logging::event(
                    "warn",
                    "dashboard.tray_start_fallback",
                    json!({"grace_seconds":15}),
                );
                dashboard.control.show_requested();
            }
            Ok(_) => {}
        }
    }
}
