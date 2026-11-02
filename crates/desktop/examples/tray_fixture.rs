//! Private-bus SNI host for native tray lifecycle regression tests; never install.
struct Watcher;
#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    fn register_status_notifier_item(&self, _service: &str) {}
    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _connection = zbus::connection::Builder::session()?
        .name("org.kde.StatusNotifierWatcher")?
        .serve_at("/StatusNotifierWatcher", Watcher)?
        .build()
        .await?;
    std::future::pending::<()>().await;
    Ok(())
}
