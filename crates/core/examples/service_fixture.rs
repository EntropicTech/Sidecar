//! Private-session integration fixture, never installed or used by the product.
use anyhow::{ensure, Context, Result};
use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
};
struct OwnedChild(Arc<Mutex<Option<Child>>>);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.lock().expect("fixture child lock").take() {
            if child.try_wait().expect("fixture child status").is_none() {
                child.kill().expect("stop owned fixture collector");
            }
            child.wait().expect("reap owned fixture collector");
        }
    }
}
struct Manager {
    binary: PathBuf,
    child: Arc<Mutex<Option<Child>>>,
}
#[zbus::interface(name = "org.freedesktop.systemd1.Manager")]
impl Manager {
    fn start_unit(
        &self,
        unit: &str,
        mode: &str,
    ) -> zbus::fdo::Result<zbus::zvariant::OwnedObjectPath> {
        if unit != "igpu-man.service" || mode != "fail" {
            return Err(zbus::fdo::Error::InvalidArgs(
                "Unexpected fixture unit request".into(),
            ));
        }
        let mut child = self
            .child
            .lock()
            .map_err(|_| zbus::fdo::Error::Failed("Fixture lock failed".into()))?;
        if child.is_some() {
            return Err(zbus::fdo::Error::Failed(
                "Duplicate fixture service start".into(),
            ));
        }
        *child = Some(
            Command::new(&self.binary)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| zbus::fdo::Error::Failed("Fixture collector start failed".into()))?,
        );
        Ok(
            zbus::zvariant::OwnedObjectPath::try_from("/org/freedesktop/systemd1/job/1")
                .expect("fixed fixture path"),
        )
    }
}
fn main() -> Result<()> {
    let binary = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .context("Fixture collector path required")?,
    );
    ensure!(
        binary.is_absolute(),
        "Use an absolute fixture collector path"
    );
    let child = Arc::new(Mutex::new(None));
    let _owned = OwnedChild(child.clone());
    let connection = zbus::blocking::connection::Builder::session()?
        .name("org.freedesktop.systemd1")?
        .serve_at("/org/freedesktop/systemd1", Manager { binary, child })?
        .build()?;
    // A private bus must never replace an existing manager/collector.
    let proxy = zbus::blocking::Proxy::new(
        &connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )?;
    let collector: bool = proxy.call("NameHasOwner", &("io.github.igpu_man.Collector",))?;
    ensure!(!collector, "Existing fixture collector left untouched");
    println!("ready");
    io::stdout().flush()?;
    for line in io::stdin().lock().lines() {
        if line? == "quit" {
            break;
        }
    }
    Ok(())
}
