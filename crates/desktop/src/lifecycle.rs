//! Fast GUI control is independent of collector RPCs and rendering workload.
use super::Action;
use eframe::egui;
use igpu_core::logging;
use serde_json::json;
use std::{
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};

pub const BUS: &str = igpu_core::DASHBOARD_BUS;
pub const OBJECT: &str = igpu_core::DASHBOARD_OBJECT;
pub const INTERFACE: &str = igpu_core::DASHBOARD_INTERFACE;

#[derive(Default)]
struct State {
    ready: bool,
    quitting: bool,
    wants_open: bool,
    visible: bool,
    view: usize,
    updates: u64,
    windows_created: u64,
    active_trial: bool,
    tray: bool,
    telemetry_ns: u64,
    worker_health: Option<(&'static str, &'static str, bool)>,
}

#[derive(Clone)]
pub struct Control {
    sender: mpsc::Sender<Action>,
    context: Arc<Mutex<Option<egui::Context>>>,
    state: Arc<Mutex<State>>,
}
impl Control {
    pub fn send(&self, action: Action) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state.quitting {
            return false;
        }
        let quitting = matches!(&action, Action::Quit);
        if matches!(&action, Action::Show | Action::ShowApplications) {
            state.wants_open = true;
        }
        if self.sender.send(action).is_err() {
            return false;
        }
        // Acknowledge shutdown intent before returning the D-Bus reply. A Show
        // arriving before the next UI frame must not be accepted and then lost.
        state.quitting |= quitting;
        drop(state);
        if quitting {
            logging::event("info", "dashboard.quit_requested", json!({}));
        }
        logging::event("debug", "dashboard.control_enqueued", json!({}));
        self.request_repaint();
        true
    }
    pub fn attach(&self, context: egui::Context) {
        if let Ok(mut current) = self.context.lock() {
            *current = Some(context);
        }
        if let Ok(mut state) = self.state.lock() {
            state.windows_created += 1;
        }
    }
    pub fn detach(&self) {
        if let Ok(mut current) = self.context.lock() {
            *current = None;
        }
    }
    pub fn request_repaint(&self) {
        let current = self.context.lock().ok().and_then(|ctx| ctx.clone());
        if let Some(ctx) = current {
            ctx.request_repaint();
        }
    }
    pub fn hide_requested(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.wants_open = false;
        }
    }
    pub fn show_requested(&self) {
        if let Ok(mut state) = self.state.lock() {
            if !state.quitting {
                state.wants_open = true;
            }
        }
    }
    pub fn tray_state(&self, available: bool) {
        if let Ok(mut state) = self.state.lock() {
            state.ready = true;
            state.tray = available;
        }
    }
    pub fn parked(&self) {
        self.detach();
        if let Ok(mut state) = self.state.lock() {
            state.visible = false;
            state.telemetry_ns = 0;
            state.updates += 1;
        }
    }
    pub fn wants_open(&self) -> bool {
        self.state
            .lock()
            .map(|s| s.wants_open && !s.quitting)
            .unwrap_or(false)
    }
    /// Seal no-tray exit atomically so a concurrent Show cannot be lost.
    pub fn finish_close(&self, tray: bool) -> bool {
        let Ok(mut state) = self.state.lock() else {
            return true;
        };
        if !tray && !state.wants_open {
            state.quitting = true;
        }
        state.quitting
    }
    pub fn sender(&self) -> mpsc::Sender<Action> {
        self.sender.clone()
    }
    pub fn update(
        &self,
        quitting: bool,
        visible: bool,
        active_trial: bool,
        tray: bool,
        telemetry_ns: u64,
    ) {
        if let Ok(mut state) = self.state.lock() {
            state.ready = true;
            state.quitting |= quitting;
            state.visible = visible;
            state.active_trial = active_trial;
            state.tray = tray;
            state.telemetry_ns = telemetry_ns;
            state.updates += 1;
        }
    }
    pub fn selected_view(&self, view: usize) {
        if let Ok(mut state) = self.state.lock() {
            state.view = view;
        }
    }
    pub fn worker_health(
        &self,
        telemetry: &'static str,
        recording: &'static str,
        backend_healthy: bool,
    ) {
        if let Ok(mut state) = self.state.lock() {
            state.worker_health = Some((telemetry, recording, backend_healthy));
        }
    }
    fn status(&self) -> zbus::fdo::Result<String> {
        let state = self
            .state
            .lock()
            .map_err(|_| zbus::fdo::Error::Failed("GUI lifecycle lock poisoned".into()))?;
        Ok(json!({"schema_version":1,"source_sha256":igpu_core::build::SOURCE_ID,"pid":std::process::id(),"ready":state.ready,"quitting":state.quitting,
            "windows_created":state.windows_created,"visible":state.visible,"view":state.view,"tray_resident":state.ready&&!state.visible&&!state.quitting&&state.tray,"updates":state.updates,"active_trial":state.active_trial,"tray_available":state.tray,"telemetry_monotonic_ns":state.telemetry_ns,"worker_health":state.worker_health}).to_string())
    }
}

struct Service(Control);
#[zbus::interface(name = "io.github.igpu_man.Dashboard1")]
impl Service {
    fn show(&self, applications: bool) -> bool {
        self.0.send(if applications {
            Action::ShowApplications
        } else {
            Action::Show
        })
    }
    fn close(&self) -> bool {
        self.0.send(Action::Close)
    }
    fn quit(&self) -> bool {
        self.0.send(Action::Quit)
    }
    fn status(&self) -> zbus::fdo::Result<String> {
        self.0.status()
    }
}

pub struct Session {
    pub _connection: zbus::blocking::Connection,
    pub control: Control,
    pub events: mpsc::Receiver<Action>,
    pub start_in_tray: bool,
}

#[derive(clap::Parser)]
#[command(name="igpu-man-desktop",version,long_version=igpu_core::build::LONG_VERSION,about="Sidecar GPU placement dashboard and tray",group(clap::ArgGroup::new("mode").args(["applications","status","close","quit","tray"])))]
struct Options {
    /// Start in the tray without opening a window; leave an existing instance alone.
    #[arg(long)]
    tray: bool,
    /// Open Applications in the existing or new dashboard.
    #[arg(long)]
    applications: bool,
    /// Print dashboard state without opening a window.
    #[arg(long)]
    status: bool,
    /// Close the window to the tray (exit if the tray is unavailable).
    #[arg(long)]
    close: bool,
    /// Quit the existing dashboard and tray.
    #[arg(long)]
    quit: bool,
}

/// None means a command was delivered to an existing process (or no GUI was running).
pub fn start_or_forward(args: &[String]) -> zbus::Result<Option<Session>> {
    use clap::Parser;
    let Options {
        status,
        close,
        quit,
        applications,
        tray,
    } = Options::try_parse_from(
        std::iter::once("igpu-man-desktop").chain(args.iter().map(String::as_str)),
    )
    .unwrap_or_else(|error| {
        if error.use_stderr() {
            logging::event(
                "warn",
                "dashboard.arguments_rejected",
                json!({"kind":format!("{:?}",error.kind())}),
            );
        }
        error.exit()
    });
    let (sender, events) = mpsc::channel();
    let control = Control {
        sender,
        context: Arc::new(Mutex::new(None)),
        state: Arc::new(Mutex::new(State::default())),
    };
    let connection = zbus::blocking::connection::Builder::session()?
        .method_timeout(Duration::from_secs(2))
        .serve_at(OBJECT, Service(control.clone()))?
        .build()?;
    // Inspection/control helpers must never temporarily own the singleton name:
    // otherwise a status probe can race a real cold start and receive its Show.
    if status || close || quit {
        let bus = zbus::blocking::Proxy::new(
            &connection,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
        )?;
        let exists: bool = bus.call("NameHasOwner", &(BUS))?;
        if !exists {
            if status {
                println!("{}", json!({"schema_version":1,"running":false}));
            }
            return Ok(None);
        }
        let proxy = zbus::blocking::Proxy::new(&connection, BUS, OBJECT, INTERFACE)?;
        if status {
            let value: String = proxy.call("Status", &())?;
            println!("{value}");
        } else {
            let _: bool = if quit {
                proxy.call("Quit", &())?
            } else {
                proxy.call("Close", &())?
            };
        }
        return Ok(None);
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match connection
            .request_name_with_flags(BUS, zbus::fdo::RequestNameFlags::DoNotQueue.into())
        {
            Ok(_) => {
                logging::event("info", "dashboard.instance_acquired", json!({}));
                return Ok(Some(Session {
                    _connection: connection,
                    control,
                    events,
                    start_in_tray: tray,
                }));
            }
            Err(zbus::Error::NameTaken) => {
                if tray {
                    return Ok(None);
                } // Login startup must never reveal/hide a running UI.
                let proxy = zbus::blocking::Proxy::new(&connection, BUS, OBJECT, INTERFACE)?;
                let accepted: bool = match proxy.call("Show", &(applications)) {
                    Ok(accepted) => accepted,
                    // The old owner can disappear between RequestName and Show.
                    // Show is idempotent UI control; never apply this retry to
                    // recording/profile mutations.
                    Err(zbus::Error::MethodError(name, _, _))
                        if [
                            "org.freedesktop.DBus.Error.ServiceUnknown",
                            "org.freedesktop.DBus.Error.NameHasNoOwner",
                            "org.freedesktop.DBus.Error.UnknownObject",
                            "org.freedesktop.DBus.Error.NoReply",
                        ]
                        .contains(&name.as_str()) =>
                    {
                        false
                    }
                    Err(error) => return Err(error),
                };
                if accepted {
                    logging::event(
                        "info",
                        "dashboard.command_forwarded",
                        json!({"command":"show"}),
                    );
                    return Ok(None);
                }
                // A rapid reopen can meet the old instance as it is shutting down.
                if Instant::now() >= deadline {
                    return Err(zbus::Error::Failure(
                        "Dashboard is still shutting down; retry shortly".into(),
                    ));
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => return Err(error),
        }
    }
}

/// A cold parked Quit can return before D-Bus writes its method reply. Drain
/// outstanding handlers, bounded so a broken/malicious peer cannot prevent exit.
pub fn drain_connection(connection: zbus::blocking::Connection) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();
    match runtime {
        Ok(runtime) => {
            if runtime
                .block_on(async {
                    tokio::time::timeout(
                        Duration::from_secs(1),
                        connection.into_inner().graceful_shutdown(),
                    )
                    .await
                })
                .is_err()
            {
                logging::event("warn", "dashboard.control_drain_timeout", json!({}));
            }
        }
        Err(error) => logging::event(
            "warn",
            "dashboard.control_drain_failed",
            logging::error_fields(&error.into()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Control, mpsc::Receiver<Action>) {
        let (sender, receiver) = mpsc::channel();
        (
            Control {
                sender,
                context: Arc::new(Mutex::new(None)),
                state: Arc::new(Mutex::new(State::default())),
            },
            receiver,
        )
    }
    #[test]
    fn close_can_be_followed_by_show_without_losing_reopen() {
        let (control, _receiver) = fixture();
        assert!(control.send(Action::Close));
        control.hide_requested();
        assert!(control.send(Action::Show));
        assert!(!control.finish_close(false));
        assert!(control.wants_open());
    }
    #[test]
    fn missing_tray_seals_exit_but_available_tray_accepts_reopen() {
        let (control, _receiver) = fixture();
        control.hide_requested();
        assert!(!control.finish_close(true));
        assert!(control.send(Action::Show));
        control.hide_requested();
        assert!(control.finish_close(false));
        assert!(!control.send(Action::Show));
    }
    #[test]
    fn queued_quit_rejects_reopen_before_the_next_frame() {
        let (sender, receiver) = mpsc::channel();
        let control = Control {
            sender,
            context: Arc::new(Mutex::new(None)),
            state: Arc::new(Mutex::new(State::default())),
        };
        assert!(control.send(Action::Quit));
        assert!(!control.send(Action::Show));
        control.update(false, true, false, false, 0);
        assert!(!control.send(Action::ShowApplications));
        assert!(matches!(receiver.try_recv(), Ok(Action::Quit)));
        assert!(receiver.try_recv().is_err());
    }
}
