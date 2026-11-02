use eframe::egui;
use igpu_core::api::Operation;
use igpu_core::{
    client,
    config::{Config, Paths},
    launch,
    model::*,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};
mod applications_ui;
mod backend;
mod branding;
mod graphs;
mod lifecycle;
mod recording_worker;
mod resident;
mod worker_health;
use backend::{backend_failure, backend_recovered, Backend};
mod theme;
use applications_ui::{Trial, TrialSetup};

enum Action {
    Rpc(&'static str, Value),
    Show,
    ShowApplications,
    Close,
    Mark,
    Quit,
    RefreshLaunchers,
    TrialKept,
    TrayChanged,
    StartTrial {
        profile: Box<Profile>,
        previous: Vec<ProcessId>,
    },
}

struct Tray {
    sender: mpsc::Sender<Action>,
    control: lifecycle::Control,
    icons: Vec<ksni::Icon>,
    available: Arc<std::sync::atomic::AtomicBool>,
    visible: Arc<std::sync::atomic::AtomicBool>,
    watcher_seen: Arc<Mutex<bool>>,
}

impl ksni::Tray for Tray {
    fn watcher_online(&self) {
        if let Ok(mut seen) = self.watcher_seen.lock() {
            *seen = true;
            self.available
                .store(true, std::sync::atomic::Ordering::Release);
        }
        self.control.send(Action::TrayChanged);
        self.control.request_repaint();
    }
    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
        let was_online = if let Ok(mut seen) = self.watcher_seen.lock() {
            *seen = true;
            self.available
                .swap(false, std::sync::atomic::Ordering::AcqRel)
        } else {
            false
        };
        self.control.send(Action::TrayChanged);
        if was_online && !self.visible.load(std::sync::atomic::Ordering::Acquire) {
            self.control.send(Action::Show);
        }
        true // ksni reconnects when Plasma's watcher returns.
    }
    fn id(&self) -> String {
        "igpu-man".into()
    }
    fn title(&self) -> String {
        "Sidecar · GPU workload manager".into()
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icons.clone()
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: branding::NAME.into(),
            description: "GPU placement and recordings".into(),
            ..Default::default()
        }
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        vec![
            StandardItem {
                label: "Open dashboard".into(),
                activate: Box::new(|tray: &mut Self| {
                    tray.control.send(Action::Show);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Stutter happened — capture".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.sender.send(Action::Mark);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Stop recording".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.sender.send(Action::Rpc("stop", json!({})));
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit Sidecar".into(),
                activate: Box::new(|tray: &mut Self| {
                    tray.control.send(Action::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        self.control.send(Action::Show);
    }
}

struct Dashboard {
    backend: Arc<Mutex<Backend>>,
    sender: mpsc::Sender<Action>,
    telemetry_health: worker_health::Health,
    recording_health: worker_health::Health,
    recording_sender: mpsc::SyncSender<recording_worker::Job>,
    ui_events: mpsc::Receiver<Action>,
    control: lifecycle::Control,
    tab: usize,
    history: BTreeMap<String, VecDeque<(u64, f64)>>,
    last_sample: u64,
    auto_scale_graphs: bool,
    trial_setup: Option<TrialSetup>,
    launcher_search: String,
    browse_launchers: bool,
    show_inactive_apps: bool,
    profile_extras: Option<Profile>,
    scroll_to_trial: bool,
    name: String,
    executable: String,
    args: Vec<String>,
    gpu: String,
    firefox: bool,
    vulkan_only: bool,
    record: bool,
    mangohud: bool,
    recording_label: String,
    selected: Vec<i64>,
    notes: String,
    export_path: String,
    import_path: String,
    import_anchor: String,
    pending_delete: Option<i64>,
    visible: bool,
    visible_flag: Arc<std::sync::atomic::AtomicBool>,
    tray_available: Arc<std::sync::atomic::AtomicBool>,
    quitting: bool,
    closing: bool,
    #[cfg(feature = "screenshot")]
    screenshot_requested: bool,
    #[cfg(feature = "screenshot")]
    screenshot_started: Instant,
}

impl Dashboard {
    fn new(
        control: lifecycle::Control,
        ui_events: mpsc::Receiver<Action>,
        icons: Vec<ksni::Icon>,
        visible: bool,
    ) -> Self {
        let initial = Backend::default();
        #[cfg(feature = "screenshot")]
        let initial = {
            let mut initial = initial;
            if visible {
                if let Ok(snapshot) = client::call::<Snapshot>("snapshot", &json!({})) {
                    initial.snapshot = Some(Arc::new(snapshot));
                    initial.last_success = Some(Instant::now());
                }
                if let Ok(profiles) = client::call("profiles", &json!({})) {
                    initial.profiles = profiles;
                }
                initial.candidates = client::call("candidates", &json!({})).unwrap_or_default();
                initial.launchers = client::call("launchers", &json!({})).unwrap_or_default();
                initial.managed_elsewhere =
                    client::call("management", &json!({})).unwrap_or_default();
                if let Ok(recordings) = client::call("recordings", &json!({})) {
                    initial.recordings = recordings;
                }
                if std::env::var("IGPU_MAN_SCREENSHOT_TAB").as_deref() == Ok("3") {
                    if let Some(recording) = initial.recordings.first() {
                        let value = json!({"id": recording.id});
                        initial.detail = client::call::<Value>("detail", &value).ok().map(Arc::new);
                        if let Ok(timeline) = client::recording_timeline(&value) {
                            initial.samples = Arc::new(timeline.samples);
                            initial.frames = Arc::new(timeline.frames);
                        }
                        let ids: Vec<_> = initial.recordings.iter().map(|r| r.id).collect();
                        initial.comparison = client::call::<Value>("compare", &json!({"ids": ids}))
                            .ok()
                            .map(Arc::new);
                    }
                }
            }
            initial
        };
        let backend = Arc::new(Mutex::new(initial));
        let (recording_sender, recording_health) =
            recording_worker::spawn(backend.clone(), control.clone());
        let recording_poll = recording_sender.clone();
        let (sender, receiver) = mpsc::channel();
        let ui_sender = control.sender();
        let shared = backend.clone();
        let context = control.clone();
        let visible_flag = Arc::new(std::sync::atomic::AtomicBool::new(visible));
        let worker_visible = visible_flag.clone();
        let tray_available = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let tray_flag = tray_available.clone();
        let tray_sender = sender.clone();
        let tray_control = control.clone();
        let tray_visible = visible_flag.clone();
        let watcher_seen = Arc::new(Mutex::new(false));
        std::thread::spawn(move || {
            use ksni::TrayMethods;
            let runtime = match tokio::runtime::Runtime::new() {
                Ok(runtime) => runtime,
                Err(error) => {
                    igpu_core::logging::event(
                        "error",
                        "dashboard.tray_runtime_failed",
                        igpu_core::logging::error_fields(&error.into()),
                    );
                    return;
                }
            };
            runtime.block_on(async move {
                let tray_control_notification = tray_control.clone();
                match (Tray {
                    sender: tray_sender,
                    control: tray_control,
                    icons,
                    available: tray_flag.clone(),
                    visible: tray_visible,
                    watcher_seen: watcher_seen.clone(),
                })
                .assume_sni_available(true)
                .spawn()
                .await
                {
                    Ok(handle) => {
                        if let Ok(seen) = watcher_seen.lock() {
                            if !*seen {
                                tray_flag.store(true, std::sync::atomic::Ordering::Release);
                            }
                        }
                        tray_control_notification.send(Action::TrayChanged);
                        eprintln!("igpu-man: tray registered");
                        igpu_core::logging::event("info", "dashboard.tray_registered", json!({}));
                        let _handle = handle;
                        std::future::pending::<()>().await;
                    }
                    Err(error) => {
                        igpu_core::logging::event(
                            "warn",
                            "dashboard.tray_unavailable",
                            json!({"reason":error.to_string()}),
                        );
                        eprintln!("Tray unavailable: {error}; closing the dashboard will exit it")
                    }
                }
            });
        });
        let telemetry_health = worker_health::spawn("telemetry", move || {
            #[cfg(feature = "screenshot")]
            if std::env::var("IGPU_MAN_SCREENSHOT_WORKER_FAILURE").as_deref() == Ok("telemetry") {
                panic!("synthetic telemetry worker failure");
            }
            let mut refreshed = Instant::now() - Duration::from_secs(10);
            let mut launchers_loaded = false;
            loop {
                match receiver.recv_timeout(Duration::from_secs(1)) {
                    Ok(Action::Rpc(method, value)) => {
                        let result = client::call::<Value>(method, &value);

                        let mut state = shared.lock().unwrap();
                        if method == "keep_trial" {
                            state.trial_keep_pending = false;
                        }
                        match result {
                            Ok(value) => {
                                if method == "keep_trial" {
                                    state.trial = None;
                                    state.workflow_notice = value["launcher_path"].as_str().map(|path| format!("Profile kept. A separate Plasma launcher is ready: {path}. Find it in the application menu and pin it for future launches."))
                                        .unwrap_or_else(|| "Profile kept. Launch it from Profiles for future iGPU launches.".into());
                                    if let Some(warning) = value["launcher_warning"].as_str() {
                                        state.workflow_notice.push(' ');
                                        state.workflow_notice.push_str(warning);
                                    }
                                    let _ = ui_sender.send(Action::TrialKept);
                                }
                                if method == "discard_trial" {
                                    state.trial = None;
                                }
                                if method == "set_management" {
                                    state.managed_elsewhere =
                                        serde_json::from_value(value.clone()).unwrap_or_default();
                                }
                                if method == "detail" {
                                    state.detail = Some(Arc::new(value.clone()));
                                }
                                if method == "compare" {
                                    state.comparison = Some(Arc::new(value.clone()));
                                }
                                state.result =
                                    serde_json::to_string_pretty(&value).unwrap_or_default();
                                state.error.clear();
                            }
                            Err(e) => {
                                igpu_core::logging::event(
                                    "warn",
                                    "dashboard.action_failed",
                                    json!({"operation":method,"error":igpu_core::logging::error_fields(&e)}),
                                );
                                state.error = format!("{e:#}");
                            }
                        }

                        refreshed = Instant::now() - Duration::from_secs(10);
                    }
                    Ok(Action::Mark) => {
                        if recording_poll
                            .try_send(recording_worker::Job::Request {
                                method: Operation::Mark,
                                value: json!({"label":"stutter happened"}),
                                generation: 0,
                            })
                            .is_err()
                        {
                            shared.lock().unwrap().error =
                                "Recording operations busy; marker was not queued.".into();
                        }
                    }
                    Ok(Action::RefreshLaunchers) => {
                        launchers_loaded = false;
                    }
                    Ok(Action::StartTrial { profile, previous }) => {
                        let result = applications_ui::start_trial(*profile, &previous);
                        let mut state = shared.lock().unwrap();
                        state.trial_pending = false;
                        match result {
                            Ok((trial, mut child)) => {
                                igpu_core::logging::event(
                                    "info",
                                    "trial.launched",
                                    json!({"process":trial.process,"gpu":trial.profile.gpu}),
                                );
                                let process = trial.process.clone();
                                state.trial = Some(trial);
                                state.error.clear();
                                state.result = "Test launched. Use the application while placement is measured.".into();
                                let shared = shared.clone();
                                std::thread::spawn(move || {
                                    let exit = child
                                        .wait()
                                        .map(|s| s.to_string())
                                        .unwrap_or_else(|e| e.to_string());
                                    igpu_core::logging::event(
                                        "info",
                                        "trial.process_exited",
                                        json!({"process":process,"exit":exit}),
                                    );
                                    if let Some(trial) = &mut shared.lock().unwrap().trial {
                                        if trial.process == process {
                                            trial.launcher_exit = Some(exit);
                                        }
                                    }
                                });
                            }
                            Err(error) => {
                                igpu_core::logging::event(
                                    "warn",
                                    "trial.launch_failed",
                                    igpu_core::logging::error_fields(&error),
                                );
                                state.error = format!("{error:#}");
                            }
                        }
                    }
                    Ok(
                        action @ (Action::Show
                        | Action::ShowApplications
                        | Action::Close
                        | Action::Quit
                        | Action::TrialKept
                        | Action::TrayChanged),
                    ) => {
                        let _ = ui_sender.send(action);
                        context.request_repaint();
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                if worker_visible.load(std::sync::atomic::Ordering::Relaxed) {
                    match client::call("candidates", &json!({})) {
                        Ok(candidates) => {
                            let mut state = shared.lock().unwrap();
                            state.candidates = candidates;
                            backend_recovered(&mut state, "candidates");
                        }
                        Err(error) => {
                            let mut state = shared.lock().unwrap();
                            state.candidates.clear();
                            backend_failure(&mut state, "candidates", &error);
                        }
                    }
                    if !launchers_loaded {
                        match client::call("launchers", &json!({})) {
                            Ok(launchers) => {
                                let mut state = shared.lock().unwrap();
                                state.launchers = launchers;
                                backend_recovered(&mut state, "launchers");
                                launchers_loaded = true;
                            }
                            Err(error) => {
                                backend_failure(&mut shared.lock().unwrap(), "launchers", &error);
                            }
                        }
                    }
                    let process = shared
                        .lock()
                        .unwrap()
                        .trial
                        .as_ref()
                        .map(|t| t.process.clone());
                    if let Some(process) = process {
                        match client::call("verify_trial", &json!({"process": process})) {
                            Ok(verification) => {
                                let mut state = shared.lock().unwrap();
                                if let Some(trial) = &mut state.trial {
                                    trial.verification = Some(verification);
                                }
                                backend_recovered(&mut state, "trial verification");
                            }
                            Err(error) => {
                                let mut state = shared.lock().unwrap();
                                if let Some(trial) = &mut state.trial {
                                    trial.verification = None;
                                }
                                backend_failure(&mut state, "trial verification", &error);
                            }
                        }
                    }
                }
                if !worker_visible.load(std::sync::atomic::Ordering::Acquire) {
                    continue; // No dashboard telemetry/catalog polling while parked.
                }
                match client::call::<Snapshot>("snapshot", &json!({})) {
                    Ok(snapshot) => {
                        let mut state = shared.lock().unwrap();
                        if snapshot.api_version == igpu_core::API_VERSION {
                            state.snapshot = Some(Arc::new(snapshot));
                            state.last_success = Some(Instant::now());
                            state.connection_error.clear();
                            backend_recovered(&mut state, "snapshot");
                        } else {
                            state.connection_error = "Incompatible collector API".into();
                        }
                    }
                    Err(error) => {
                        let mut state = shared.lock().unwrap();
                        state.connection_error = format!("{error:#}");
                        backend_failure(&mut state, "snapshot", &error);
                    }
                }
                if refreshed.elapsed() > Duration::from_secs(5) {
                    match client::call("profiles", &json!({})) {
                        Ok(profiles) => {
                            let mut state = shared.lock().unwrap();
                            state.profiles = profiles;
                            backend_recovered(&mut state, "profiles");
                        }
                        Err(error) => {
                            backend_failure(&mut shared.lock().unwrap(), "profiles", &error);
                        }
                    }
                    let _ = recording_poll.try_send(recording_worker::Job::Poll); // Coalesced polling; skip if user jobs occupy the bounded queue.
                    match client::call("management", &json!({})) {
                        Ok(rules) => {
                            let mut state = shared.lock().unwrap();
                            state.managed_elsewhere = rules;
                            backend_recovered(&mut state, "management");
                        }
                        Err(error) => {
                            backend_failure(&mut shared.lock().unwrap(), "management", &error);
                        }
                    }
                    match client::call("pending_trials", &json!({})) {
                        Ok(trials) => {
                            let mut state = shared.lock().unwrap();
                            state.pending_trials = trials;
                            backend_recovered(&mut state, "pending trials");
                        }
                        Err(error) => {
                            let mut state = shared.lock().unwrap();
                            state.pending_trials.clear();
                            backend_failure(&mut state, "pending trials", &error);
                        }
                    }
                    refreshed = Instant::now();
                }
                if worker_visible.load(std::sync::atomic::Ordering::Relaxed) {
                    context.request_repaint();
                }
            }
        });
        let dashboard = Self {
            backend,
            sender,
            recording_sender,
            telemetry_health,
            recording_health,
            ui_events,
            control,
            tab: if cfg!(feature = "screenshot") {
                std::env::var("IGPU_MAN_SCREENSHOT_TAB")
                    .ok()
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(0)
                    .min(3)
            } else {
                usize::from(std::env::args().any(|a| a == "--applications"))
            },
            history: BTreeMap::new(),
            last_sample: 0,
            auto_scale_graphs: false,
            trial_setup: None,
            launcher_search: String::new(),
            browse_launchers: false,
            show_inactive_apps: false,
            profile_extras: None,
            scroll_to_trial: false,
            name: String::new(),
            executable: "firefox".into(),
            args: Vec::new(),
            gpu: "igpu".into(),
            firefox: true,
            vulkan_only: false,
            record: false,
            mangohud: false,
            recording_label: "browser-idle".into(),
            selected: Vec::new(),
            notes: String::new(),
            export_path: String::new(),
            import_path: String::new(),
            import_anchor: String::new(),
            pending_delete: None,
            visible,
            visible_flag,
            tray_available,
            quitting: false,
            closing: false,
            #[cfg(feature = "screenshot")]
            screenshot_requested: false,
            #[cfg(feature = "screenshot")]
            screenshot_started: Instant::now(),
        };
        #[cfg(feature = "screenshot")]
        let mut dashboard = dashboard;
        #[cfg(feature = "screenshot")]
        dashboard.initialize_screenshot_workflow();
        #[cfg(feature = "screenshot")]
        if let Some(id) = std::env::var("IGPU_MAN_SCREENSHOT_RECORDING_ID")
            .ok()
            .and_then(|s| s.parse::<i64>().ok())
        {
            dashboard.rpc("detail", json!({"id":id}));
        }
        #[cfg(feature = "screenshot")]
        if std::env::var("IGPU_MAN_SCREENSHOT_SCALE").as_deref() == Ok("auto") {
            dashboard.auto_scale_graphs = true;
        }
        dashboard
    }
    #[cfg(feature = "screenshot")]
    fn capture_frame(&mut self, ctx: &egui::Context) {
        if let Ok(path) = std::env::var("IGPU_MAN_SCREENSHOT_TO") {
            let captured = ctx.input(|input| {
                input.events.iter().find_map(|event| {
                    if let egui::Event::Screenshot { image, .. } = event {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
            });
            if let Some(image) = captured {
                let pixels: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                if let Err(e) = image::save_buffer(
                    &path,
                    &pixels,
                    image.width() as u32,
                    image.height() as u32,
                    image::ColorType::Rgba8,
                ) {
                    eprintln!("Screenshot failed: {e}");
                }
                self.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else if !self.screenshot_requested
                && ctx.cumulative_pass_nr() >= 2
                && self.screenshot_started.elapsed()
                    >= Duration::from_millis(
                        std::env::var("IGPU_MAN_SCREENSHOT_WAIT_MS")
                            .ok()
                            .and_then(|v| v.parse::<u64>().ok())
                            .unwrap_or(0)
                            .min(10_000),
                    )
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.screenshot_requested = true;
            }
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
    fn rpc(&self, method: &'static str, value: Value) {
        if let Some(method) = recording_worker::operation(method) {
            let mut state = self.backend.lock().unwrap();
            if method == Operation::Detail {
                state.recording_generation += 1;
                state.detail = None;
                state.samples = Arc::default();
                state.frames = Arc::default();
            }
            let generation = state.recording_generation;
            if self
                .recording_sender
                .try_send(recording_worker::Job::Request {
                    method,
                    value,
                    generation,
                })
                .is_err()
            {
                state.error =
                    "Recording operations busy; wait for the current request before retrying."
                        .into();
            }
            return;
        }
        if self.sender.send(Action::Rpc(method, value)).is_err() {
            self.backend.lock().unwrap().error =
                "The background worker stopped; restart the dashboard and inspect its logs.".into();
            igpu_core::logging::event(
                "error",
                "dashboard.worker_unavailable",
                json!({"operation":method}),
            );
        }
    }
    fn overview(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        ui.heading("Know where your GPU work lands");
        ui.label("Requested placement, observed activity, and measured game performance are separate facts.");
        ui.horizontal_wrapped(|ui| {
            ui.label("Live chart scale");
            ui.selectable_value(&mut self.auto_scale_graphs, false, "Fixed 0–100%");
            ui.selectable_value(&mut self.auto_scale_graphs, true, "Auto-scale")
                .on_hover_text("Each GPU uses its own zero-based scale for the visible two-minute history, including peaks. This changes only the chart axis.");
        });
        if self.auto_scale_graphs {
            ui.label(egui::RichText::new("Auto-scale is independent per GPU. Compare the percentage readings and labeled axes, rather than line heights.").small().color(theme::MUTED));
        }
        ui.add_space(12.0);
        ui.columns(snapshot.gpus.len().clamp(1, 2), |columns| {
            for (index, gpu) in snapshot.gpus.iter().enumerate() {
                let count = columns.len();
                let ui = &mut columns[index % count];
                let color = theme::gpu_color(gpu);
                egui::Frame::new()
                    .fill(theme::SURFACE)
                    .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                    .corner_radius(10)
                    .inner_margin(14)
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        let (bar, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 3.0),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect_filled(bar, 2.0, color);
                        ui.horizontal(|ui| {
                            let title = match gpu.alias.as_str() {
                                "igpu" => "iGPU",
                                "dgpu" => "dGPU",
                                other => other,
                            };
                            ui.heading(egui::RichText::new(title).color(color));
                            theme::badge(
                                ui,
                                if gpu.integrated {
                                    "Integrated GPU"
                                } else {
                                    "Dedicated GPU"
                                },
                                color,
                            );
                        });
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), 34.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_min_height(34.0);
                                ui.small(&gpu.name);
                            },
                        );
                        ui.label(
                            egui::RichText::new(&gpu.pci)
                                .monospace()
                                .color(theme::MUTED),
                        );
                        if let Some(node) = &gpu.render_node {
                            ui.label(egui::RichText::new(node).small().color(theme::MUTED));
                        }
                        ui.add_space(4.0);
                        ui.columns(3, |stats| {
                            for (i, (key, title)) in [
                                ("gpu_busy", "GPU busy"),
                                ("power", "Power"),
                                ("temperature", "Temperature"),
                            ]
                            .iter()
                            .enumerate()
                            {
                                stats[i]
                                    .label(egui::RichText::new(*title).small().color(theme::MUTED));
                                if let Some(metric) = gpu.metrics.get(*key) {
                                    stats[i]
                                        .label(
                                            egui::RichText::new(if metric.value.is_some() {
                                                metric_text(metric)
                                            } else {
                                                "—".into()
                                            })
                                            .size(if i == 0 { 28.0 } else { 22.0 })
                                            .strong()
                                            .color(
                                                theme::metric_color(
                                                    metric,
                                                    if i == 0 { color } else { theme::TEXT },
                                                ),
                                            ),
                                        )
                                        .on_hover_text(format!(
                                            "Source: {}\nStatus: {:?}",
                                            metric.source, metric.status
                                        ));
                                    if metric.value.is_none() {
                                        stats[i].label(
                                            egui::RichText::new(metric_text(metric))
                                                .small()
                                                .color(theme::MUTED),
                                        );
                                    }
                                } else {
                                    stats[i].label(
                                        egui::RichText::new("—").size(22.0).color(theme::MUTED),
                                    );
                                }
                            }
                        });
                        let empty = VecDeque::new();
                        chart(
                            ui,
                            self.history.get(&gpu.pci).unwrap_or(&empty),
                            color,
                            snapshot.monotonic_ns,
                            self.auto_scale_graphs,
                        );
                        for (name, metric) in &gpu.metrics {
                            if !["gpu_busy", "power", "temperature"].contains(&name.as_str()) {
                                metric_row(ui, name, metric);
                            }
                        }
                        for output in gpu.outputs.iter().filter(|o| o.connected) {
                            ui.separator();
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} · {} · {} Hz · scale {}",
                                    output.connector,
                                    output
                                        .current_mode
                                        .as_deref()
                                        .unwrap_or("current mode unavailable"),
                                    output
                                        .refresh_hz
                                        .map(|n| format!("{n:.2}"))
                                        .unwrap_or_else(|| "—".into()),
                                    output
                                        .scale
                                        .map(|n| format!("{n:.2}"))
                                        .unwrap_or_else(|| "—".into())
                                ))
                                .color(color),
                            );
                        }
                    });
            }
        });
        if snapshot.gpus.is_empty() {
            ui.label("No accessible AMD DRM devices found. Run igpu-man doctor for details.");
        }
        ui.add_space(12.0);
        ui.heading("Collection health");
        ui.label(format!("{} accessible user processes · {} unique DRM clients · {} permission failures · {:.2} ms per sample",
            snapshot.coverage.processes_inspected, snapshot.coverage.clients_observed, snapshot.coverage.permission_denied, snapshot.collector_duration_ms));
        for note in &snapshot.diagnostics.notes {
            ui.small(note);
        }
        for recommendation in &snapshot.recommendations {
            ui.colored_label(theme::WARNING, recommendation);
        }
        ui.collapsing("KWin and display diagnostics", |ui| {
            ui.label("Transfers between GPUs are not directly measured. Monitor attachment does not prove application rendering placement.");
            ui.monospace(snapshot.diagnostics.kwin_support.as_deref().unwrap_or("KWin diagnostics unavailable"));
        });
    }
    fn application_details(&self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        ui.heading("Detailed application metrics");
        ui.label("Engine percentages measure time active, not share of GPU performance. Shared clients are counted once; client memory is not a device total.");
        for app in &snapshot.applications {
            ui.push_id((&app.id.pid, &app.id.start_ticks), |ui| {
                ui.collapsing(format!("{} [{}] · {} · CPU {}", app.name, app.id.pid, app.placement, metric_text(&app.cpu_percent)), |ui| {
                    ui.monospace(&app.executable);
                    ui.label(format!("Requested: {} · observed active: {}", app.requested_gpu.as_deref().unwrap_or("unspecified"), app.observed_gpus.join(", ")));
                    for (pci, usage) in &app.gpus {
                        ui.strong(format!("{pci} · device opened: {}", usage.opened));
                        if usage.engines.is_empty() { ui.label("No readable engine accounting. An open device does not establish activity."); }
                        for (engine, metric) in &usage.engines { metric_row(ui, engine, metric); }
                        ui.collapsing("Client memory accounting (shared allocations may overlap)", |ui| { for (region, metric) in &usage.memory { metric_row(ui, region, metric); } });
                    }
                    ui.collapsing("Member processes", |ui| {
                        for process in &app.processes { ui.label(format!("{} · {} · CPU {}", process.id.pid, process.name, metric_text(&process.cpu_percent))); }
                    });
                });
            });
        }
        ui.separator();
        ui.heading("Firefox video checklist");
        for item in ["Fully exit Firefox before applying a different GPU launch profile.",
            "Open about:support and check Graphics/WebRender and Media codec support.",
            "Play a known video; inspect graphics and video/decode engines separately above.",
            "Watch CPU activity and note visible playback drops in your recording notes.",
            "Missing decoder activity does not prove software decoding. Playback drops are not automatically measured in this release."] { ui.label(format!("• {item}")); }
    }
    fn profiles(&mut self, ui: &mut egui::Ui, profiles: &[Profile], snapshot: Option<&Snapshot>) {
        ui.heading("Explicit launch profiles");
        ui.label("Profiles apply when an application starts. Running applications keep their existing settings.");
        for profile in profiles {
            ui.horizontal_wrapped(|ui| {
                ui.strong(&profile.name); ui.label(format!("{} to {}", profile.executable, profile.gpu));
                if ui.button("Edit").clicked() {
                    self.profile_extras = Some(profile.clone());
                    self.name = profile.name.clone(); self.executable = profile.executable.clone(); self.args = profile.args.clone();
                    self.gpu = profile.gpu.clone(); self.firefox = profile.firefox; self.vulkan_only = profile.vulkan_only;
                    self.record = profile.record; self.mangohud = profile.mangohud;
                }
                if ui.add(theme::action("Launch", theme::IGPU)).clicked() {
                    let result = if (profile.firefox || launch::is_firefox(&profile.executable)) && launch::firefox_running(std::path::Path::new("/proc"), std::process::id()) {
                        Err("Firefox is already running. Fully exit it before launching a different GPU profile.".into())
                    } else { launch_cli(&["launch", &profile.name]) };
                    if let Err(e) = result { self.backend.lock().unwrap().error = e; }
                }
                if ui.button("Create launcher").clicked() {
                    let result = (|| -> anyhow::Result<()> {
                        let paths = Paths::discover()?; std::fs::create_dir_all(&paths.applications)?;
                        let cli = cli_path();
                        std::fs::write(paths.applications.join(format!("igpu-man-{}.desktop", profile.name)), launch::desktop_entry(profile, &cli)?)?;
                        Ok(())
                    })();
                    let mut backend = self.backend.lock().unwrap();
                    match result { Ok(()) => backend.result = "Desktop launcher created".into(), Err(e) => backend.error = format!("{e:#}") }
                }
                if ui.button("Delete profile").clicked() { self.rpc("delete_profile", json!({"name": profile.name})); }
            });
        }
        ui.separator();
        egui::Grid::new("profile-editor")
            .num_columns(2)
            .spacing([16.0, 8.0])
            .show(ui, |ui| {
                ui.label("Profile name");
                ui.text_edit_singleline(&mut self.name);
                ui.end_row();
                ui.label("Executable");
                ui.text_edit_singleline(&mut self.executable);
                ui.end_row();
                ui.label("Arguments");
                ui.vertical(|ui| {
                    let mut remove = None;
                    for (index, argument) in self.args.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.push_id(index, |ui| {
                                ui.text_edit_singleline(argument);
                            });
                            if ui.button("Remove").clicked() {
                                remove = Some(index);
                            }
                        });
                    }
                    if let Some(index) = remove {
                        self.args.remove(index);
                    }
                    if ui.button("Add argument").clicked() {
                        self.args.push(String::new());
                    }
                });
                ui.end_row();
                ui.label("GPU");
                egui::ComboBox::from_id_salt("target-gpu")
                    .selected_text(&self.gpu)
                    .show_ui(ui, |ui| {
                        if let Some(snapshot) = snapshot {
                            for gpu in &snapshot.gpus {
                                ui.selectable_value(
                                    &mut self.gpu,
                                    gpu.pci.clone(),
                                    format!("{} · {}", gpu.alias, gpu.pci),
                                );
                            }
                        }
                    });
                ui.end_row();
            });
        ui.checkbox(
            &mut self.firefox,
            "Firefox adapter (explicit render device; requires fresh launch)",
        );
        ui.checkbox(&mut self.vulkan_only, "Expose only the selected Vulkan GPU");
        ui.checkbox(
            &mut self.record,
            "Start a telemetry recording with this launch",
        );
        ui.add_enabled_ui(self.record, |ui| {
            ui.checkbox(
                &mut self.mangohud,
                "MangoHud frame log (first 120 seconds, native games; no gamescope)",
            );
        });
        if !self.record {
            self.mangohud = false;
        }
        if ui.add(theme::action("Save profile", theme::DGPU)).clicked() {
            let profile = Profile {
                name: self.name.clone(),
                executable: self.executable.clone(),
                args: self.args.clone(),
                gpu: self.gpu.clone(),
                firefox: self.firefox,
                vulkan_only: self.vulkan_only,
                record: self.record,
                mangohud: self.mangohud,
                ..self.profile_extras.clone().unwrap_or_default()
            };
            self.rpc("save_profile", serde_json::to_value(profile).unwrap());
        }
        if let Some(snapshot) = snapshot {
            for gpu in &snapshot.gpus {
                ui.horizontal(|ui| {
                    ui.label(format!("Steam · {}", gpu.alias));
                    if ui.button("Copy launch command").clicked() {
                        ui.ctx().copy_text(launch::steam_command(gpu));
                    }
                });
            }
        }
    }
    fn recordings(&mut self, ui: &mut egui::Ui, recordings: &[Recording]) {
        ui.heading("Recordings and experiments");
        if self.backend.lock().unwrap().recording_busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Recording work in progress; live monitoring continues.");
            });
        }
        ui.label("Use the same scene, video, and settings for each run. Label repeated runs with the same condition to compare their variation.");
        ui.horizontal_wrapped(|ui| {
            ui.text_edit_singleline(&mut self.recording_label);
            if ui
                .add(theme::action("Start recording", theme::MARKER))
                .clicked()
            {
                self.rpc("start", json!({"label": self.recording_label}));
            }
            if ui
                .add(theme::action("Stop recording", theme::WARNING))
                .clicked()
            {
                self.rpc("stop", json!({}));
            }
            if ui
                .add(theme::action("Mark stutter", theme::WARNING))
                .clicked()
            {
                self.rpc("mark", json!({"label": "stutter happened"}));
            }
            if ui.button("Experiment schedule").clicked() {
                self.rpc("experiment", json!({}));
            }
        });
        for recording in recordings {
            ui.horizontal_wrapped(|ui| {
                let mut selected = self.selected.contains(&recording.id);
                if ui
                    .checkbox(
                        &mut selected,
                        format!(
                            "#{} {} · {} · {} samples",
                            recording.id, recording.label, recording.state, recording.samples
                        ),
                    )
                    .changed()
                {
                    self.selected.retain(|&id| id != recording.id);
                    if selected {
                        self.selected.push(recording.id);
                    }
                }
                if ui.button("Inspect").clicked() {
                    self.notes = recording.notes.clone();
                    self.rpc("detail", json!({"id": recording.id}));
                }
                if ui.button("Delete…").clicked() {
                    self.pending_delete = Some(recording.id);
                }
            });
        }
        if let Some(id) = self.pending_delete {
            ui.horizontal(|ui| {
                ui.colored_label(
                    theme::WARNING,
                    format!("Permanently delete recording #{id}?"),
                );
                if ui.add(theme::action("Delete", theme::DANGER)).clicked() {
                    self.rpc("delete_recording", json!({"id": id}));
                    self.selected.retain(|&n| n != id);
                    self.pending_delete = None;
                }
                if ui.button("Cancel").clicked() {
                    self.pending_delete = None;
                }
            });
        }
        if self.selected.len() >= 2
            && ui
                .add(theme::action("Compare selected recordings", theme::DGPU))
                .clicked()
        {
            self.rpc("compare", json!({"ids": self.selected}));
        }
        if self.selected.len() == 1 {
            let id = self.selected[0];
            ui.separator();
            ui.label(format!("Selected recording #{id}"));
            ui.label("Playback quality / experiment notes");
            ui.text_edit_multiline(&mut self.notes);
            if ui.button("Save notes").clicked() {
                self.rpc("notes", json!({"id": id, "notes": self.notes}));
            }
            ui.horizontal(|ui| {
                ui.label("Export to new absolute directory");
                ui.text_edit_singleline(&mut self.export_path);
            });
            if ui.button("Export bundle").clicked() {
                self.rpc("export", json!({"id": id, "destination": self.export_path}));
            }
            ui.horizontal(|ui| {
                ui.label("MangoHud CSV path");
                ui.text_edit_singleline(&mut self.import_path);
            });
            ui.horizontal(|ui| {
                ui.label("Optional elapsed=0 monotonic anchor (ns)");
                ui.text_edit_singleline(&mut self.import_anchor);
            });
            if ui.button("Import frame log").clicked() {
                let anchor = if self.import_anchor.trim().is_empty() {
                    Ok(None)
                } else {
                    self.import_anchor.parse::<u64>().map(Some)
                };
                match anchor {
                    Ok(anchor) => {
                        if self
                            .recording_sender
                            .try_send(recording_worker::Job::Import {
                                id,
                                path: self.import_path.clone().into(),
                                anchor,
                            })
                            .is_err()
                        {
                            self.backend.lock().unwrap().error =
                                "Recording operations busy; retry after completion.".into();
                        }
                    }
                    Err(_) => {
                        self.backend.lock().unwrap().error =
                            "Alignment anchor must be an unsigned integer.".into()
                    }
                }
            }
        }
        let (detail, comparison, samples, frames) = {
            let backend = self.backend.lock().unwrap();
            (
                backend.detail.clone(),
                backend.comparison.clone(),
                backend.samples.clone(),
                backend.frames.clone(),
            )
        };
        if let Some(detail) = detail {
            ui.separator();
            ui.heading(format!(
                "Recording #{} · {}",
                detail["recording"]["id"],
                detail["recording"]["label"].as_str().unwrap_or("")
            ));
            if let Some(logs) = detail["frame_logs"].as_array() {
                if !logs.is_empty() {
                    ui.small("Frametime trace shows bucket maxima; statistics use every frame.");
                }
                for log in logs {
                    ui.label(format!(
                        "{} · {}",
                        log["filename"].as_str().unwrap_or("frame log"),
                        if log["aligned"].as_bool() == Some(true) {
                            "estimated alignment; see uncertainty"
                        } else {
                            "unaligned — summary only"
                        }
                    ));
                    summary_table(ui, &log["summary"], &format!("summary-{}", log["filename"]));
                }
            }
            let markers: Vec<Marker> =
                serde_json::from_value(detail["recording"]["markers"].clone()).unwrap_or_default();
            timeline(ui, &samples, &frames, &markers);
        }
        if let Some(comparison) = comparison {
            ui.separator();
            ui.heading("Comparison");
            if let Some(warnings) = comparison["mismatch_warnings"].as_array() {
                for warning in warnings {
                    ui.colored_label(
                        theme::WARNING,
                        warning.as_str().unwrap_or("Metadata mismatch"),
                    );
                }
            }
            if let Some(recordings) = comparison["recordings"].as_array() {
                for recording in recordings {
                    ui.strong(format!(
                        "#{} {}",
                        recording["recording"]["id"],
                        recording["recording"]["label"].as_str().unwrap_or("")
                    ));
                    if let Some(logs) = recording["frame_logs"].as_array() {
                        for (index, log) in logs.iter().enumerate() {
                            summary_table(
                                ui,
                                &log["summary"],
                                &format!("comparison-{}-{index}", recording["recording"]["id"]),
                            );
                        }
                    }
                }
            }
            ui.label("Run-to-run variation by condition (min–max)");
            if let Some(conditions) = comparison["run_variation_by_label"].as_object() {
                for (label, range) in conditions {
                    ui.label(format!(
                        "{label} · {} runs · average FPS {:.1}–{:.1} · p99 {:.2}–{:.2} ms",
                        range["runs"],
                        range["average_fps_range"]["min"].as_f64().unwrap_or(0.0),
                        range["average_fps_range"]["max"].as_f64().unwrap_or(0.0),
                        range["p99_ms_range"]["min"].as_f64().unwrap_or(0.0),
                        range["p99_ms_range"]["max"].as_f64().unwrap_or(0.0)
                    ));
                }
            }
            ui.small("Observed differences are not a causal finding or a statistical significance claim. Check playback-quality notes and metadata.");
        }
    }
}

impl eframe::App for Dashboard {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // A worker can poison the mutex after the early health check. Abandon
        // that frame and use the recovery view; never read poisoned data.
        // Unrelated panics still terminate normally through the panic hook.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.update_inner(ctx, frame)
        }));
        if let Err(payload) = result {
            if self.backend.is_poisoned() {
                igpu_core::logging::event("error", "dashboard.backend_poisoned", json!({}));
                ctx.request_repaint();
            } else {
                std::panic::resume_unwind(payload);
            }
        }
    }
}
impl Dashboard {
    fn update_inner(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting && !self.closing {
            self.control.hide_requested();
            self.closing = true;
            igpu_core::logging::event("info", "dashboard.window_close_requested", json!({}));
        }
        self.control.selected_view(self.tab);
        let unhealthy = self.backend.is_poisoned()
            || self.telemetry_health.failed()
            || self.recording_health.failed();
        self.control.worker_health(
            self.telemetry_health.label(),
            self.recording_health.label(),
            !self.backend.is_poisoned(),
        );
        if unhealthy {
            while let Ok(event) = self.ui_events.try_recv() {
                match event {
                    Action::Quit => self.quitting = true,
                    Action::Close => {
                        self.control.hide_requested();
                        self.closing = true;
                    }
                    Action::Show | Action::ShowApplications => {
                        self.control.show_requested();
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                            egui::UserAttentionType::Informational,
                        ));
                    }
                    _ => {}
                }
            }
            self.control.update(
                self.quitting,
                !self.quitting && !self.closing,
                false,
                self.tray_available
                    .load(std::sync::atomic::Ordering::Relaxed),
                0,
            );
            if self.quitting || self.closing {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.heading("Dashboard background work stopped");
                ui.label("The collector and running applications continue. Quit and relaunch Sidecar to restart its workers; saved profiles and recordings are preserved.");
                ui.label(format!("Telemetry: {} · Recordings: {}",self.telemetry_health.label(),self.recording_health.label()));
                ui.label("Inspect the private desktop log for dashboard.worker_failed or runtime.panic.");
                if ui.button("Quit Sidecar").clicked(){self.control.send(Action::Quit);}
            });
            #[cfg(feature = "screenshot")]
            self.capture_frame(ctx);
            ctx.request_repaint_after(Duration::from_secs(1));
            return;
        }
        #[cfg(feature = "screenshot")]
        self.capture_frame(ctx);
        while let Ok(event) = self.ui_events.try_recv() {
            match event {
                Action::TrialKept => {
                    self.trial_setup = None;
                }
                action @ (Action::Show | Action::ShowApplications) => {
                    self.control.show_requested();
                    if matches!(action, Action::ShowApplications) {
                        self.tab = 1;
                    }
                    self.visible = true;
                    self.visible_flag
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                        egui::UserAttentionType::Informational,
                    ));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                Action::Quit => {
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Action::Close => {
                    self.control.hide_requested();
                    self.closing = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                _ => {}
            }
        }
        let (active_trial, telemetry_ns) = {
            let state = self.backend.lock().unwrap();
            (
                state.trial.is_some(),
                state.snapshot.as_ref().map(|s| s.monotonic_ns).unwrap_or(0),
            )
        };
        self.control.update(
            self.quitting,
            !self.quitting && !self.closing && self.visible,
            active_trial,
            self.tray_available
                .load(std::sync::atomic::Ordering::Relaxed),
            telemetry_ns,
        );
        if self.quitting || self.closing {
            igpu_core::logging::event(
                "info",
                "dashboard.closing",
                json!({"quit":self.quitting,"tray_available":self.tray_available.load(std::sync::atomic::Ordering::Acquire)}),
            );
            self.visible_flag
                .store(false, std::sync::atomic::Ordering::Relaxed);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if !self.visible {
            return;
        }
        let (snapshot, profiles, recordings, error, result, stale) = {
            let backend = self.backend.lock().unwrap();
            (
                backend.snapshot.clone(),
                backend.profiles.clone(),
                backend.recordings.clone(),
                [backend.connection_error.as_str(), backend.error.as_str()]
                    .into_iter()
                    .chain(
                        backend
                            .health_errors
                            .iter()
                            .filter(|(key, _)| **key != "snapshot")
                            .map(|(_, message)| message.as_str()),
                    )
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n"),
                backend.result.clone(),
                backend
                    .last_success
                    .is_none_or(|t| t.elapsed() > Duration::from_secs(3)),
            )
        };
        if let Some(snapshot) = &snapshot {
            if snapshot.monotonic_ns != self.last_sample {
                self.last_sample = snapshot.monotonic_ns;
                for gpu in &snapshot.gpus {
                    let history = self.history.entry(gpu.pci.clone()).or_default();
                    if let Some(value) = gpu.metrics.get("gpu_busy").and_then(|m| m.value) {
                        history.push_back((snapshot.monotonic_ns, value));
                    }
                    while history.front().is_some_and(|(time, _)| {
                        snapshot.monotonic_ns.saturating_sub(*time) > 120_000_000_000
                    }) {
                        history.pop_front();
                    }
                }
            }
        }
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let (indicator, _) =
                    ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter()
                    .circle_filled(indicator.center(), 4.0, theme::IGPU);
                ui.heading(
                    egui::RichText::new(branding::NAME)
                        .color(theme::TEXT)
                        .strong(),
                );
                ui.label(egui::RichText::new("GPU workload manager").color(theme::MUTED));
                if stale {
                    theme::badge(ui, "Disconnected / stale", theme::WARNING);
                } else {
                    theme::badge(ui, "Collector connected", theme::IGPU);
                }
                if let Some(id) = snapshot.as_ref().and_then(|s| s.active_recording) {
                    theme::badge(ui, &format!("● Recording #{id}"), theme::MARKER);
                }
            });
            ui.horizontal(|ui| {
                for (index, name) in ["Overview", "Applications", "Profiles", "Recordings"]
                    .iter()
                    .enumerate()
                {
                    ui.selectable_value(&mut self.tab, index, *name);
                }
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                if !error.is_empty() {
                    ui.colored_label(theme::DANGER, &error);
                }
                match self.tab {
                    0 => {
                        if let Some(snapshot) = &snapshot {
                            self.overview(ui, snapshot);
                        } else {
                            ui.label(
                                "Start the collector: systemctl --user start igpu-man.service",
                            );
                        }
                    }
                    1 => {
                        if let Some(snapshot) = &snapshot {
                            self.applications(ui, snapshot);
                        }
                    }
                    2 => self.profiles(ui, &profiles, snapshot.as_deref()),
                    _ => self.recordings(ui, &recordings),
                }
                if !result.is_empty() {
                    ui.separator();
                    ui.collapsing("Last operation / comparison result", |ui| {
                        ui.monospace(&result);
                    });
                }
            });
        });
    }
}

fn metric_text(metric: &Metric) -> String {
    match metric.value {
        Some(value) if metric.unit.starts_with("bytes") => {
            let (value, unit) = if value >= 1024_f64.powi(3) {
                (value / 1024_f64.powi(3), "GiB")
            } else if value >= 1024_f64.powi(2) {
                (value / 1024_f64.powi(2), "MiB")
            } else if value >= 1024.0 {
                (value / 1024.0, "KiB")
            } else {
                (value, "B")
            };
            format!(
                "{value:.2} {unit}{}",
                if metric.unit.contains("client") {
                    " (client; shared)"
                } else {
                    ""
                }
            )
        }
        Some(value) if metric.unit == "W" && value != 0.0 && value.abs() < 1.0 => {
            format!("{:.1} mW", value * 1000.0)
        }
        Some(value) => format!("{value:.1} {}", metric.unit),
        None => format!(
            "— ({})",
            match metric.status {
                Availability::Available => "available",
                Availability::Idle => "idle",
                Availability::Unavailable => "unavailable",
                Availability::PermissionDenied => "permission denied",
                Availability::Stale => "stale",
                Availability::WarmingUp => "warming up",
            }
        ),
    }
}
fn summary_table(ui: &mut egui::Ui, summary: &Value, id: &str) {
    egui::Grid::new(id).striped(true).show(ui, |ui| {
        for label in [
            "Frames",
            "Average FPS",
            "Median ms",
            "p95 ms",
            "p99 ms",
            "Over threshold",
        ] {
            ui.strong(label);
        }
        ui.end_row();
        ui.label(summary["frames"].to_string());
        for field in ["average_fps", "median_ms", "p95_ms", "p99_ms"] {
            ui.label(format!("{:.2}", summary[field].as_f64().unwrap_or(0.0)));
        }
        ui.label(format!(
            "{} (> {} ms)",
            summary["frames_over_threshold"], summary["threshold_ms"]
        ));
        ui.end_row();
    });
}
fn timeline(
    ui: &mut egui::Ui,
    samples: &[TimelineSample],
    logs: &[FrameSeries],
    markers: &[Marker],
) {
    let (Some(first), Some(last)) = (samples.first(), samples.last()) else {
        return;
    };
    let start = first.monotonic_ns;
    let span = last.monotonic_ns.saturating_sub(start).max(1) as f64;
    ui.strong("Recorded timeline");
    ui.horizontal_wrapped(|ui| {
        for gpu in &first.gpus {
            theme::badge(ui, &gpu.alias, theme::gpu_color(gpu));
        }
        if logs.iter().any(|log| log.anchor_ns.is_some()) {
            theme::badge(ui, "Frametime", theme::WARNING);
        }
        if !markers.is_empty() {
            theme::badge(ui, "Event markers", theme::MARKER);
        }
    });
    ui.label(
        egui::RichText::new(if logs.iter().any(|log| log.anchor_ns.is_some()) {
            "GPU busy: 0–100% · aligned frametime: 0–100 ms (clipped)"
        } else {
            "GPU busy: 0–100% · no aligned frame log"
        })
        .small()
        .color(theme::MUTED),
    );
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 185.0),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    let plot = egui::Rect::from_min_max(
        rect.min + egui::vec2(39.0, 14.0),
        rect.max - egui::vec2(15.0, 27.0),
    );
    theme::plot_background(&painter, rect, plot, 100.0);
    for step in 0..=4 {
        painter.text(
            egui::pos2(
                egui::lerp(plot.left()..=plot.right(), step as f32 / 4.0),
                plot.bottom() + 13.0,
            ),
            egui::Align2::CENTER_CENTER,
            if span < 1e9 {
                format!("{:.1}ms", span / 1e6 * step as f64 / 4.0)
            } else {
                format!("{:.1}s", span / 1e9 * step as f64 / 4.0)
            },
            egui::FontId::monospace(10.0),
            theme::MUTED,
        );
    }
    let position = |time: u64, value: f64| {
        egui::pos2(
            plot.left() + ((time.saturating_sub(start) as f64 / span) as f32) * plot.width(),
            plot.bottom() - (value as f32 / 100.0).clamp(0.0, 1.0) * plot.height(),
        )
    };
    for gpu in &first.gpus {
        let points: Vec<_> = samples
            .iter()
            .filter_map(|s| {
                s.gpus
                    .iter()
                    .find(|g| g.pci == gpu.pci)
                    .and_then(|g| g.metrics.get("gpu_busy"))
                    .and_then(|m| m.value)
                    .map(|v| position(s.monotonic_ns, v))
            })
            .collect();
        theme::trace(&painter, &points, theme::gpu_color(gpu), None);
    }
    for log in logs {
        let Some(anchor) = log.anchor_ns else {
            continue;
        };
        let points: Vec<_> = log
            .frames
            .iter()
            .filter_map(|f| {
                anchor
                    .checked_add(f.elapsed_ns)
                    .filter(|&t| t >= start && t <= last.monotonic_ns)
                    .map(|t| position(t, f.frametime_ms))
            })
            .collect();
        theme::trace(&painter, &points, theme::WARNING, None);
        ui.small(format!(
            "{} alignment uncertainty: {} ms; estimated, not proof of causation",
            log.filename,
            log.uncertainty_ms
                .map(|v| format!("{v:.1}"))
                .unwrap_or_else(|| "unknown".into())
        ));
    }
    if markers.len() > 2000 {
        ui.small("Showing the first 2,000 markers; the export retains every marker.");
    }
    for marker in markers
        .iter()
        .take(2000)
        .filter(|m| m.monotonic_ns >= start && m.monotonic_ns <= last.monotonic_ns)
    {
        let x = position(marker.monotonic_ns, 0.0).x;
        painter.line_segment(
            [egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())],
            egui::Stroke::new(1.5_f32, theme::MARKER),
        );
        painter.circle_filled(egui::pos2(x, plot.top()), 3.0, theme::MARKER);
    }
    if let Some(pointer) = response.hover_pos().filter(|p| plot.contains(*p)) {
        let time = start + ((pointer.x - plot.left()) / plot.width() * span as f32) as u64;
        painter.line_segment(
            [
                egui::pos2(pointer.x, plot.top()),
                egui::pos2(pointer.x, plot.bottom()),
            ],
            egui::Stroke::new(1.0_f32, theme::MUTED),
        );
        let nearby = markers
            .iter()
            .filter(|m| (m.monotonic_ns.abs_diff(time) as f64) < span * 0.01)
            .map(|m| m.label.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        response.on_hover_text(format!(
            "{:.3} seconds since first telemetry sample\n{}",
            time.saturating_sub(start) as f64 / 1e9,
            nearby
        ));
    }
}
fn metric_row(ui: &mut egui::Ui, label: &str, metric: &Metric) {
    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new(match label {
                "gpu_busy" => "GPU busy",
                "memory_busy" => "Memory busy",
                "core_clock" => "Core clock",
                "memory_clock" => "Memory clock",
                "vram_total" => "VRAM total",
                "vram_used" => "VRAM used",
                "gtt_used" => "System memory (GTT)",
                "temperature" => "Temperature",
                "power" => "Power",
                other => other,
            })
            .color(theme::MUTED),
        );
        ui.label(
            egui::RichText::new(metric_text(metric))
                .monospace()
                .strong()
                .color(theme::metric_color(metric, theme::TEXT)),
        )
        .on_hover_text(format!(
            "Source: {}\nStatus: {:?}",
            metric.source, metric.status
        ));
    });
}
fn chart(
    ui: &mut egui::Ui,
    values: &VecDeque<(u64, f64)>,
    color: egui::Color32,
    now: u64,
    auto: bool,
) {
    let ceiling = graphs::utilization_ceiling(values, now, auto);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("GPU utilization").strong());
        theme::badge(
            ui,
            &format!(
                "{} · 0–{}%",
                if auto { "Auto" } else { "Fixed" },
                graphs::tick_label(ceiling)
            ),
            color,
        );
        ui.label(egui::RichText::new("2 min").small().color(theme::MUTED));
    });
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 166.0),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    let plot = egui::Rect::from_min_max(
        rect.min + egui::vec2(39.0, 14.0),
        rect.max - egui::vec2(15.0, 27.0),
    );
    theme::plot_background(&painter, rect, plot, ceiling);
    for (step, label) in ["−2m", "−90s", "−60s", "−30s", "now"].iter().enumerate() {
        painter.text(
            egui::pos2(
                egui::lerp(plot.left()..=plot.right(), step as f32 / 4.0),
                plot.bottom() + 13.0,
            ),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::monospace(10.0),
            theme::MUTED,
        );
    }
    let position = |time: u64, value: f64| {
        egui::pos2(
            plot.right() - (now.saturating_sub(time) as f32 / 120e9) * plot.width(),
            plot.bottom() - (value / ceiling).clamp(0.0, 1.0) as f32 * plot.height(),
        )
    };
    let mut segment = Vec::new();
    let mut previous = None;
    for &(time, value) in values
        .iter()
        .filter(|(time, value)| *time <= now && now - time <= 120_000_000_000 && value.is_finite())
    {
        if previous.is_some_and(|old| time.saturating_sub(old) > 3_000_000_000) {
            theme::trace(&painter, &segment, color, Some(plot.bottom()));
            segment.clear();
        }
        segment.push(position(time, value));
        previous = Some(time);
    }
    theme::trace(&painter, &segment, color, Some(plot.bottom()));
    if let Some(&(time, value)) = values
        .back()
        .filter(|(time, _)| now.saturating_sub(*time) <= 3_000_000_000)
    {
        let point = position(time, value);
        painter.circle_filled(point, 5.0, theme::tint(color, 20));
        painter.circle_filled(point, 2.5, color);
    }
    if let Some(pointer) = response.hover_pos().filter(|p| plot.contains(*p)) {
        let wanted = now.saturating_sub(((plot.right() - pointer.x) / plot.width() * 120e9) as u64);
        painter.line_segment(
            [
                egui::pos2(pointer.x, plot.top()),
                egui::pos2(pointer.x, plot.bottom()),
            ],
            egui::Stroke::new(1.0_f32, theme::MUTED),
        );
        if let Some(&(time, value)) = values
            .iter()
            .min_by_key(|(time, _)| time.abs_diff(wanted))
            .filter(|(time, _)| time.abs_diff(wanted) <= 3_000_000_000)
        {
            response.on_hover_text(format!(
                "GPU busy: {value:.1}%\n{:.1} seconds ago · kernel device sensor",
                now.saturating_sub(time) as f64 / 1e9
            ));
        } else {
            response.on_hover_text("No sample at this time");
        }
    }
    if values.len() < 2 {
        ui.label(
            egui::RichText::new("Collecting history…")
                .small()
                .color(theme::MUTED),
        );
    }
}
fn cli_path() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("igpu-man")))
        .filter(|p| p.exists())
        .unwrap_or_else(|| "igpu-man".into())
}
fn launch_cli(args: &[&str]) -> Result<(), String> {
    std::process::Command::new(cli_path())
        .args(args)
        .spawn()
        .map(|mut child| {
            std::thread::spawn(move || {
                if let Err(error) = child.wait() {
                    igpu_core::logging::event(
                        "warn",
                        "launch.reap_failed",
                        igpu_core::logging::error_fields(&error.into()),
                    );
                }
            });
        })
        .map_err(|e| e.to_string())
}

fn main() -> std::process::ExitCode {
    igpu_core::logging::init("desktop");
    let result = run();
    if let Err(error) = &result {
        igpu_core::logging::event(
            "error",
            "dashboard.failed",
            igpu_core::logging::error_fields(&anyhow::anyhow!(error.to_string())),
        );
    }
    if let Err(error) = result {
        eprintln!(
            "igpu-man-desktop: {}",
            igpu_core::logging::diagnostic_message(&anyhow::anyhow!(error.to_string()))
        );
        std::process::ExitCode::FAILURE
    } else {
        std::process::ExitCode::SUCCESS
    }
}

fn run() -> eframe::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let Some(session) =
        lifecycle::start_or_forward(&args).map_err(|e| eframe::Error::AppCreation(Box::new(e)))?
    else {
        return Ok(());
    };
    let lifecycle::Session {
        control,
        events,
        _connection,
        start_in_tray,
    } = session;
    // Configure before starting threads or creating the graphics context.
    if std::env::var_os("DRI_PRIME").is_none() {
        let config = Paths::discover()
            .and_then(|p| Config::load(&p))
            .map_err(|error| {
                eframe::Error::AppCreation(
                    anyhow::anyhow!(igpu_core::logging::diagnostic_message(&error)).into(),
                )
            })?;
        if let Some(gpu) =
            igpu_core::collector::discover_gpus(std::path::Path::new("/sys"), &config)
                .iter()
                .find(|g| g.integrated)
        {
            if let Ok(environment) = launch::environment(gpu, false, false) {
                for (key, value) in environment {
                    std::env::set_var(key, value);
                }
            }
        }
    }
    let (window_icon, tray_icons) = branding::load()?;
    let result = resident::run(control, events, window_icon, tray_icons, start_in_tray);
    lifecycle::drain_connection(_connection);
    result
}

#[cfg(test)]
mod backend_tests {
    use super::*;
    #[test]
    fn failed_recording_fetch_does_not_reuse_the_previous_timeline() {
        let mut state = Backend {
            frames: Arc::new(vec![FrameSeries {
                total_frames: 0,
                aggregation: String::new(),
                filename: "previous.csv".into(),
                anchor_ns: None,
                uncertainty_ms: None,
                frames: vec![],
            }]),
            ..Default::default()
        };
        state.apply_timeline(Err(anyhow::anyhow!("request timed out")), Ok(vec![]));
        assert!(state.frames.is_empty() && state.samples.is_empty());
        assert!(state.error.contains("Timeline unavailable"));
    }
}
